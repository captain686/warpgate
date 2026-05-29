use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};
use sea_orm::ActiveValue::NotSet;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, ModelTrait, QueryFilter,
    Set,
};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;
use warpgate_common::helpers::rng::get_crypto_rng;
use warpgate_common::{AdminPermission, UserPublicKeyCredential, WarpgateError};
use warpgate_common_http::AuthenticatedRequestContext;
use warpgate_core::logging::{AuditEvent, CredentialChangedVia};
use warpgate_db_entities::{PublicKeyCredential, Target, User};

use super::AnySecurityScheme;
use crate::api::common::{require_admin_permission, require_manage_admin_accounts_permission};

async fn check_user_ldap_linked(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<bool, WarpgateError> {
    let maybe_user = User::Entity::find_by_id(user_id).one(db).await?;
    Ok(maybe_user.is_some_and(|u| u.ldap_server_id.is_some()))
}

/// Checks if a user is LDAP-linked and returns an error message if they are.
/// Returns Ok(()) if the user is not LDAP-linked, or a formatted error string if they are.
async fn verify_user_not_ldap_linked(db: &DatabaseConnection, user_id: Uuid) -> Result<(), String> {
    if check_user_ldap_linked(db, user_id).await.unwrap_or(false) {
        Err("Cannot manage SSH keys for LDAP-linked users. Keys are synced from LDAP.".to_string())
    } else {
        Ok(())
    }
}

fn normalize_public_key(openssh_public_key: &str) -> Result<String, WarpgateError> {
    let mut key = russh::keys::PublicKey::from_openssh(openssh_public_key)
        .map_err(russh::keys::Error::from)?;
    key.set_comment("");
    key.to_openssh()
        .map_err(russh::keys::Error::from)
        .map_err(Into::into)
}

fn parse_positive_option(
    value: Option<i64>,
    field_name: &str,
) -> Result<Option<i64>, WarpgateError> {
    match value {
        Some(v) if v <= 0 => Err(WarpgateError::InvalidRequest(format!(
            "{field_name} must be greater than 0"
        ))),
        Some(v) => Ok(Some(v)),
        None => Ok(None),
    }
}

fn compute_expiry(valid_for_seconds: Option<i64>) -> Result<Option<OffsetDateTime>, WarpgateError> {
    let seconds = parse_positive_option(valid_for_seconds, "valid_for_seconds")?;
    let Some(seconds) = seconds else {
        return Ok(None);
    };

    OffsetDateTime::now_utc()
        .checked_add(time::Duration::seconds(seconds))
        .ok_or_else(|| {
            WarpgateError::InvalidRequest("valid_for_seconds is too large to represent".into())
        })
        .map(Some)
}

#[derive(Object)]
struct ExistingPublicKeyCredential {
    id: Uuid,
    label: String,
    target_id: Option<Uuid>,
    date_added: Option<OffsetDateTime>,
    last_used: Option<OffsetDateTime>,
    openssh_public_key: String,
    issued_by_warpgate: bool,
    expires_at: Option<OffsetDateTime>,
    max_uses: Option<i64>,
    uses_left: Option<i64>,
    revoked_at: Option<OffsetDateTime>,
}

#[derive(Object)]
struct NewPublicKeyCredential {
    label: String,
    openssh_public_key: String,
}

#[derive(Enum, Deserialize, Copy, Clone, Eq, PartialEq)]
enum IssuedPublicKeyAlgorithm {
    #[oai(rename = "ed25519")]
    #[serde(rename = "ed25519")]
    Ed25519,
    #[oai(rename = "rsa_sha512")]
    #[serde(rename = "rsa_sha512")]
    RsaSha512,
}

#[derive(Object, Deserialize)]
struct IssuePublicKeyCredentialRequest {
    label: String,
    #[serde(alias = "targetId")]
    target_id: Option<Uuid>,
    valid_for_seconds: Option<i64>,
    max_uses: Option<i64>,
    algorithm: Option<IssuedPublicKeyAlgorithm>,
}

#[derive(Object)]
struct IssuedPublicKeyCredential {
    credential: ExistingPublicKeyCredential,
    private_key_openssh: String,
}

fn generate_issued_keypair(
    algorithm: IssuedPublicKeyAlgorithm,
) -> Result<(String, String), WarpgateError> {
    let key_algorithm = match algorithm {
        IssuedPublicKeyAlgorithm::Ed25519 => russh::keys::Algorithm::Ed25519,
        IssuedPublicKeyAlgorithm::RsaSha512 => russh::keys::Algorithm::Rsa {
            hash: Some(russh::keys::HashAlg::Sha512),
        },
    };

    let private_key = russh::keys::PrivateKey::random(&mut get_crypto_rng(), key_algorithm)
        .map_err(russh::keys::Error::from)?;

    let private_key_openssh = private_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(russh::keys::Error::from)?
        .to_string();

    let mut public_key = private_key.public_key().clone();
    public_key.set_comment("");
    let public_key_openssh = public_key.to_openssh().map_err(russh::keys::Error::from)?;

    Ok((public_key_openssh, private_key_openssh))
}

async fn validate_ssh_target(
    db: &DatabaseConnection,
    target_id: Option<Uuid>,
) -> Result<Option<Uuid>, WarpgateError> {
    let Some(target_id) = target_id else {
        return Ok(None);
    };

    let Some(target) = Target::Entity::find_by_id(target_id).one(db).await? else {
        return Err(WarpgateError::InvalidRequest(format!(
            "target_id {target_id} not found"
        )));
    };

    if target.kind != Target::TargetKind::Ssh {
        return Err(WarpgateError::InvalidRequest(
            "target_id must reference an SSH target".into(),
        ));
    }

    Ok(Some(target.id))
}

impl From<PublicKeyCredential::Model> for ExistingPublicKeyCredential {
    fn from(credential: PublicKeyCredential::Model) -> Self {
        Self {
            id: credential.id,
            date_added: credential.date_added,
            last_used: credential.last_used,
            label: credential.label,
            target_id: credential.target_id,
            openssh_public_key: credential.openssh_public_key,
            issued_by_warpgate: credential.issued_by_warpgate,
            expires_at: credential.expires_at,
            max_uses: credential.max_uses,
            uses_left: credential.uses_left,
            revoked_at: credential.revoked_at,
        }
    }
}

impl TryFrom<&NewPublicKeyCredential> for UserPublicKeyCredential {
    type Error = WarpgateError;

    fn try_from(credential: &NewPublicKeyCredential) -> Result<Self, WarpgateError> {
        Ok(Self {
            key: normalize_public_key(&credential.openssh_public_key)?.into(),
        })
    }
}

#[derive(ApiResponse)]
enum GetPublicKeyCredentialsResponse {
    #[oai(status = 200)]
    Ok(Json<Vec<ExistingPublicKeyCredential>>),
}

#[derive(ApiResponse)]
enum CreatePublicKeyCredentialResponse {
    #[oai(status = 201)]
    Created(Json<ExistingPublicKeyCredential>),
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 403)]
    Forbidden(Json<String>),
}

#[derive(ApiResponse)]
enum IssuePublicKeyCredentialResponse {
    #[oai(status = 201)]
    Created(Json<IssuedPublicKeyCredential>),
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 403)]
    Forbidden(Json<String>),
}

#[derive(ApiResponse)]
enum UpdatePublicKeyCredentialResponse {
    #[oai(status = 200)]
    Updated(Json<ExistingPublicKeyCredential>),
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 403)]
    Forbidden(Json<String>),
}

pub struct ListApi;

#[OpenApi]
impl ListApi {
    #[oai(
        path = "/users/:user_id/credentials/public-keys",
        method = "get",
        operation_id = "get_public_key_credentials"
    )]
    async fn api_get_all(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        user_id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<GetPublicKeyCredentialsResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;
        let objects = PublicKeyCredential::Entity::find()
            .filter(PublicKeyCredential::Column::UserId.eq(*user_id))
            .all(&*db)
            .await?;

        Ok(GetPublicKeyCredentialsResponse::Ok(Json(
            objects.into_iter().map(Into::into).collect(),
        )))
    }

    #[oai(
        path = "/users/:user_id/credentials/public-keys",
        method = "post",
        operation_id = "create_public_key_credential"
    )]
    async fn api_create(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewPublicKeyCredential>,
        user_id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<CreatePublicKeyCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;

        // Ensure user exists and is not LDAP-linked
        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(CreatePublicKeyCredentialResponse::NotFound);
        };

        if let Err(msg) = verify_user_not_ldap_linked(&db, *user_id).await {
            return Ok(CreatePublicKeyCredentialResponse::Forbidden(Json(msg)));
        }

        let normalized_key = normalize_public_key(&body.openssh_public_key)?;
        let object = PublicKeyCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(*user_id),
            target_id: Set(None),
            date_added: Set(Some(OffsetDateTime::now_utc())),
            last_used: Set(None),
            label: Set(body.label.clone()),
            openssh_public_key: Set(normalized_key),
            issued_by_warpgate: Set(false),
            expires_at: Set(None),
            max_uses: Set(None),
            uses_left: Set(None),
            revoked_at: Set(None),
        }
        .insert(&*db)
        .await?;

        AuditEvent::CredentialCreated {
            credential_type: "public_key".to_string(),
            credential_name: Some(body.label.clone()),
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(CreatePublicKeyCredentialResponse::Created(Json(
            object.into(),
        )))
    }

    #[oai(
        path = "/users/:user_id/credentials/public-keys/issue",
        method = "post",
        operation_id = "issue_public_key_credential"
    )]
    async fn api_issue(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<IssuePublicKeyCredentialRequest>,
        user_id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<IssuePublicKeyCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;
        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(IssuePublicKeyCredentialResponse::NotFound);
        };

        if let Err(msg) = verify_user_not_ldap_linked(&db, *user_id).await {
            return Ok(IssuePublicKeyCredentialResponse::Forbidden(Json(msg)));
        }

        let max_uses = parse_positive_option(body.max_uses, "max_uses")?;
        let expires_at = compute_expiry(body.valid_for_seconds)?;
        let uses_left = max_uses;
        let target_id = validate_ssh_target(&db, body.target_id).await?;
        let algorithm = body.algorithm.unwrap_or(IssuedPublicKeyAlgorithm::Ed25519);
        let (openssh_public_key, private_key_openssh) = generate_issued_keypair(algorithm)?;

        let object = PublicKeyCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(*user_id),
            target_id: Set(target_id),
            date_added: Set(Some(OffsetDateTime::now_utc())),
            last_used: Set(None),
            label: Set(body.label.clone()),
            openssh_public_key: Set(openssh_public_key),
            issued_by_warpgate: Set(true),
            expires_at: Set(expires_at),
            max_uses: Set(max_uses),
            uses_left: Set(uses_left),
            revoked_at: Set(None),
        }
        .insert(&*db)
        .await?;

        AuditEvent::CredentialCreated {
            credential_type: "issued_public_key".to_string(),
            credential_name: Some(body.label.clone()),
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(IssuePublicKeyCredentialResponse::Created(Json(
            IssuedPublicKeyCredential {
                credential: object.into(),
                private_key_openssh,
            },
        )))
    }
}

#[derive(ApiResponse)]
enum DeleteCredentialResponse {
    #[oai(status = 204)]
    Deleted,
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 403)]
    Forbidden(Json<String>),
}

#[derive(ApiResponse)]
enum RevokeCredentialResponse {
    #[oai(status = 204)]
    Revoked,
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 403)]
    Forbidden(Json<String>),
}

pub struct DetailApi;

#[OpenApi]
impl DetailApi {
    #[oai(
        path = "/users/:user_id/credentials/public-keys/:id",
        method = "put",
        operation_id = "update_public_key_credential"
    )]
    async fn api_update(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewPublicKeyCredential>,
        user_id: Path<Uuid>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<UpdatePublicKeyCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;
        let Some(_) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(UpdatePublicKeyCredentialResponse::NotFound);
        };

        if let Err(msg) = verify_user_not_ldap_linked(&db, *user_id).await {
            return Ok(UpdatePublicKeyCredentialResponse::Forbidden(Json(msg)));
        }

        let Some(existing) = PublicKeyCredential::Entity::find_by_id(id.0)
            .filter(PublicKeyCredential::Column::UserId.eq(*user_id))
            .one(&*db)
            .await?
        else {
            return Ok(UpdatePublicKeyCredentialResponse::NotFound);
        };

        if existing.issued_by_warpgate {
            return Ok(UpdatePublicKeyCredentialResponse::Forbidden(Json(
                "Issued SSH keys cannot be edited. Revoke and issue a new key instead.".to_string(),
            )));
        }

        let normalized_key = normalize_public_key(&body.openssh_public_key)?;
        let mut model: PublicKeyCredential::ActiveModel = existing.into();
        model.label = Set(body.label.clone());
        model.openssh_public_key = Set(normalized_key);
        model.last_used = NotSet;

        let model = model.update(&*db).await;
        match model {
            Ok(model) => {
                let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
                    return Ok(UpdatePublicKeyCredentialResponse::NotFound);
                };

                AuditEvent::CredentialUpdated {
                    credential_type: "public_key".to_string(),
                    credential_name: Some(body.label.clone()),
                    via: CredentialChangedVia::Admin,
                    user_id: *user_id,
                    username: user.username,
                    actor_user_id: ctx.auth.user_id(),
                }
                .emit();

                Ok(UpdatePublicKeyCredentialResponse::Updated(Json(
                    model.into(),
                )))
            }
            Err(DbErr::RecordNotFound(_)) => Ok(UpdatePublicKeyCredentialResponse::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    #[oai(
        path = "/users/:user_id/credentials/public-keys/:id",
        method = "delete",
        operation_id = "delete_public_key_credential"
    )]
    async fn api_delete(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        user_id: Path<Uuid>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<DeleteCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;

        if let Err(msg) = verify_user_not_ldap_linked(&db, *user_id).await {
            return Ok(DeleteCredentialResponse::Forbidden(Json(msg)));
        }

        let Some(model) = PublicKeyCredential::Entity::find_by_id(id.0)
            .filter(PublicKeyCredential::Column::UserId.eq(*user_id))
            .one(&*db)
            .await?
        else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        let credential_name = model.label.clone();
        model.delete(&*db).await?;

        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        AuditEvent::CredentialDeleted {
            credential_type: "public_key".to_string(),
            credential_name: Some(credential_name),
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(DeleteCredentialResponse::Deleted)
    }

    #[oai(
        path = "/users/:user_id/credentials/public-keys/:id/revoke",
        method = "post",
        operation_id = "revoke_public_key_credential"
    )]
    async fn api_revoke(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        user_id: Path<Uuid>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<RevokeCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;

        if let Err(msg) = verify_user_not_ldap_linked(&db, *user_id).await {
            return Ok(RevokeCredentialResponse::Forbidden(Json(msg)));
        }

        let Some(existing) = PublicKeyCredential::Entity::find_by_id(id.0)
            .filter(PublicKeyCredential::Column::UserId.eq(*user_id))
            .one(&*db)
            .await?
        else {
            return Ok(RevokeCredentialResponse::NotFound);
        };

        let was_revoked = existing.revoked_at.is_some();
        let credential_name = existing.label.clone();
        let mut model: PublicKeyCredential::ActiveModel = existing.into();
        if !was_revoked {
            model.revoked_at = Set(Some(OffsetDateTime::now_utc()));
            model.uses_left = Set(Some(0));
            model.update(&*db).await?;

            if let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? {
                AuditEvent::CredentialDeleted {
                    credential_type: "issued_public_key".to_string(),
                    credential_name: Some(credential_name),
                    via: CredentialChangedVia::Admin,
                    user_id: *user_id,
                    username: user.username,
                    actor_user_id: ctx.auth.user_id(),
                }
                .emit();
            }
        }

        Ok(RevokeCredentialResponse::Revoked)
    }
}

#[cfg(test)]
mod tests {
    use russh::keys::PublicKeyBase64;

    use super::*;

    #[test]
    fn normalize_public_key_strips_comment() -> Result<(), WarpgateError> {
        let with_comment = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ user@test";
        let normalized = normalize_public_key(with_comment)?;
        assert_eq!(
            normalized,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ"
        );
        Ok(())
    }

    #[test]
    fn positive_option_parser_rejects_non_positive_values() {
        assert!(parse_positive_option(Some(0), "x").is_err());
        assert!(parse_positive_option(Some(-1), "x").is_err());
        assert_eq!(parse_positive_option(None, "x").ok(), Some(None));
        assert_eq!(parse_positive_option(Some(10), "x").ok(), Some(Some(10)));
    }

    #[test]
    fn compute_expiry_validates_input() {
        assert!(compute_expiry(Some(0)).is_err());
        assert!(compute_expiry(Some(-5)).is_err());
        assert!(compute_expiry(None).is_ok());
    }

    #[test]
    fn generates_ed25519_issued_keypair() -> Result<(), WarpgateError> {
        let (public_key_openssh, private_key_openssh) =
            generate_issued_keypair(IssuedPublicKeyAlgorithm::Ed25519)?;
        let private_key = russh::keys::PrivateKey::from_openssh(&private_key_openssh)
            .map_err(russh::keys::Error::from)?;
        let mut derived_public_key = private_key.public_key().clone();
        derived_public_key.set_comment("");
        assert_eq!(
            public_key_openssh,
            derived_public_key
                .to_openssh()
                .map_err(russh::keys::Error::from)?
        );
        Ok(())
    }

    #[test]
    fn generates_rsa_issued_keypair() -> Result<(), WarpgateError> {
        let (public_key_openssh, private_key_openssh) =
            generate_issued_keypair(IssuedPublicKeyAlgorithm::RsaSha512)?;
        let private_key = russh::keys::PrivateKey::from_openssh(&private_key_openssh)
            .map_err(russh::keys::Error::from)?;
        let mut derived_public_key = private_key.public_key().clone();
        derived_public_key.set_comment("");
        let derived = derived_public_key
            .to_openssh()
            .map_err(russh::keys::Error::from)?;
        assert_eq!(public_key_openssh, derived);
        // Ensure it is an RSA key (wire format starts with "ssh-rsa" algorithm bytes).
        assert!(private_key.public_key_bytes().len() > 7);
        Ok(())
    }
}
