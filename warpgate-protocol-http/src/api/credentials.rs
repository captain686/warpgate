use http::StatusCode;
use poem::web::Data;
use poem::{Endpoint, EndpointExt, FromRequest, IntoResponse};
use poem_openapi::param::Path;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Enum, Object, OpenApi};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, QueryFilter, Set};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;
use warpgate_common::helpers::rng::get_crypto_rng;
use warpgate_common::{User, UserPasswordCredential, UserRequireCredentialsPolicy, WarpgateError};
use warpgate_common_http::auth::{AuthenticatedRequestContext, UnauthenticatedRequestContext};
use warpgate_core::ConfigProvider;
use warpgate_core::logging::{AuditEvent, CredentialChangedVia};
use warpgate_db_entities::{
    self as entities, CertificateCredential, Parameters, PasswordCredential, PublicKeyCredential,
};

use super::common::get_user;
use crate::api::AnySecurityScheme;
use crate::common::endpoint_auth;

pub struct Api;

#[derive(Enum)]
enum PasswordState {
    Unset,
    Set,
    MultipleSet,
}

#[derive(Object)]
struct ExistingSsoCredential {
    id: Uuid,
    provider: Option<String>,
    email: String,
}

impl From<entities::SsoCredential::Model> for ExistingSsoCredential {
    fn from(credential: entities::SsoCredential::Model) -> Self {
        Self {
            id: credential.id,
            provider: credential.provider,
            email: credential.email,
        }
    }
}

#[derive(Object)]
struct ChangePasswordRequest {
    password: String,
}

#[derive(ApiResponse)]
enum ChangePasswordResponse {
    #[oai(status = 201)]
    Done(Json<PasswordState>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(Object)]
pub struct CredentialsState {
    password: PasswordState,
    otp: Vec<ExistingOtpCredential>,
    public_keys: Vec<ExistingPublicKeyCredential>,
    certificates: Vec<ExistingCertificateCredential>,
    sso: Vec<ExistingSsoCredential>,
    credential_policy: UserRequireCredentialsPolicy,
    ldap_linked: bool,
}

#[derive(ApiResponse)]
#[allow(clippy::large_enum_variant)]
enum CredentialsStateResponse {
    #[oai(status = 200)]
    Ok(Json<CredentialsState>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(Object, Deserialize)]
struct NewPublicKeyCredential {
    label: String,
    openssh_public_key: String,
    #[serde(alias = "targetId")]
    target_id: Option<Uuid>,
}

#[derive(Object)]
struct ExistingPublicKeyCredential {
    id: Uuid,
    label: String,
    target_id: Option<Uuid>,
    date_added: Option<OffsetDateTime>,
    last_used: Option<OffsetDateTime>,
    abbreviated: String,
    openssh_public_key: String,
    issued_by_warpgate: bool,
    expires_at: Option<OffsetDateTime>,
    max_uses: Option<i64>,
    uses_left: Option<i64>,
    revoked_at: Option<OffsetDateTime>,
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
    target_id: Uuid,
    valid_for_seconds: Option<i64>,
    max_uses: Option<i64>,
    algorithm: Option<IssuedPublicKeyAlgorithm>,
}

#[derive(Object)]
struct IssuedPublicKeyCredential {
    credential: ExistingPublicKeyCredential,
    private_key_openssh: String,
}

fn abbreviate_public_key(k: &str) -> String {
    let l = 10;
    if k.len() <= l {
        return k.to_string(); // Return the full key if it's shorter than or equal to `l`.
    }

    format!(
        "{}...{}",
        &k[..l.min(k.len())],            // Take the first `l` characters.
        &k[k.len().saturating_sub(l)..]  // Take the last `l` characters safely.
    )
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
    let Some(seconds) = parse_positive_option(valid_for_seconds, "valid_for_seconds")? else {
        return Ok(None);
    };

    OffsetDateTime::now_utc()
        .checked_add(time::Duration::seconds(seconds))
        .ok_or_else(|| {
            WarpgateError::InvalidRequest("valid_for_seconds is too large to represent".into())
        })
        .map(Some)
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
    ctx: &AuthenticatedRequestContext,
    username: &str,
    target_id: Option<Uuid>,
) -> Result<Option<Uuid>, WarpgateError> {
    let Some(target_id) = target_id else {
        return Ok(None);
    };

    let Some(target) = ({
        let db = ctx.services().db.lock().await;
        entities::Target::Entity::find_by_id(target_id)
            .one(&*db)
            .await?
    }) else {
        return Err(WarpgateError::InvalidRequest(format!(
            "target_id {target_id} not found"
        )));
    };

    if target.kind != entities::Target::TargetKind::Ssh {
        return Err(WarpgateError::InvalidRequest(
            "target_id must reference an SSH target".into(),
        ));
    }

    let authorized = ctx
        .services()
        .config_provider
        .lock()
        .await
        .authorize_target(username, &target.name)
        .await?;

    if !authorized {
        return Err(WarpgateError::InvalidRequest(
            "target_id is not available to the current user".into(),
        ));
    }

    Ok(Some(target.id))
}

async fn current_user(
    ctx: &AuthenticatedRequestContext,
) -> Result<Option<entities::User::Model>, WarpgateError> {
    let db = ctx.services().db.lock().await;
    get_user(&ctx.auth, &db).await
}

impl From<entities::PublicKeyCredential::Model> for ExistingPublicKeyCredential {
    fn from(credential: entities::PublicKeyCredential::Model) -> Self {
        Self {
            id: credential.id,
            label: credential.label,
            target_id: credential.target_id,
            date_added: credential.date_added,
            last_used: credential.last_used,
            abbreviated: abbreviate_public_key(&credential.openssh_public_key),
            openssh_public_key: credential.openssh_public_key,
            issued_by_warpgate: credential.issued_by_warpgate,
            expires_at: credential.expires_at,
            max_uses: credential.max_uses,
            uses_left: credential.uses_left,
            revoked_at: credential.revoked_at,
        }
    }
}
#[derive(ApiResponse)]
#[allow(clippy::large_enum_variant)]
enum CreatePublicKeyCredentialResponse {
    #[oai(status = 201)]
    Created(Json<ExistingPublicKeyCredential>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(ApiResponse)]
#[allow(clippy::large_enum_variant)]
enum IssuePublicKeyCredentialResponse {
    #[oai(status = 201)]
    Created(Json<IssuedPublicKeyCredential>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(ApiResponse)]
enum DeleteCredentialResponse {
    #[oai(status = 204)]
    Deleted,
    #[oai(status = 401)]
    Unauthorized,
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum RevokeCredentialResponse {
    #[oai(status = 204)]
    Revoked,
    #[oai(status = 401)]
    Unauthorized,
    #[oai(status = 404)]
    NotFound,
}

#[derive(Object, Deserialize)]
struct NewOtpCredential {
    secret_key: Vec<u8>,
    #[serde(alias = "targetId")]
    target_id: Option<Uuid>,
}

#[derive(Object)]
struct ExistingOtpCredential {
    id: Uuid,
    target_id: Option<Uuid>,
}

impl From<entities::OtpCredential::Model> for ExistingOtpCredential {
    fn from(credential: entities::OtpCredential::Model) -> Self {
        Self {
            id: credential.id,
            target_id: credential.target_id,
        }
    }
}

#[derive(ApiResponse)]
enum CreateOtpCredentialResponse {
    #[oai(status = 201)]
    Created(Json<ExistingOtpCredential>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(Object)]
struct ExistingCertificateCredential {
    id: Uuid,
    label: String,
    date_added: Option<OffsetDateTime>,
    last_used: Option<OffsetDateTime>,
    fingerprint: String,
}

fn certificate_fingerprint(certificate_pem: &str) -> Result<String, WarpgateError> {
    Ok(warpgate_ca::certificate_sha256_hex_fingerprint(
        &warpgate_ca::deserialize_certificate(certificate_pem)?,
    )?)
}

impl From<entities::CertificateCredential::Model> for ExistingCertificateCredential {
    fn from(credential: entities::CertificateCredential::Model) -> Self {
        Self {
            id: credential.id,
            label: credential.label,
            date_added: credential.date_added,
            last_used: credential.last_used,
            fingerprint: certificate_fingerprint(&credential.certificate_pem)
                .unwrap_or_else(|_| "Invalid certificate".into()),
        }
    }
}

#[derive(Object)]
struct IssuedCertificateCredential {
    credential: ExistingCertificateCredential,
    certificate_pem: String,
}

#[derive(Object)]
struct IssueCertificateCredentialRequest {
    label: String,
    public_key_pem: String,
}

#[derive(ApiResponse)]
enum IssueCertificateCredentialResponse {
    #[oai(status = 201)]
    Issued(Json<IssuedCertificateCredential>),
    #[oai(status = 401)]
    Unauthorized,
}

#[derive(ApiResponse)]
enum DeleteCertificateCredentialResponse {
    #[oai(status = 200)]
    Ok,
    #[oai(status = 401)]
    Unauthorized,
    #[oai(status = 404)]
    NotFound,
}

pub fn parameters_based_auth<E: Endpoint + 'static>(e: E) -> impl Endpoint {
    e.around(|ep, req| async move {
        let ctx = Data::<&UnauthenticatedRequestContext>::from_request_without_body(&req).await?;
        let services = ctx.services();
        let parameters = Parameters::Entity::get(&*services.db.lock().await)
            .await
            .map_err(WarpgateError::from)?;
        if !parameters.allow_own_credential_management {
            return Ok(poem::Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body("Credential management is disabled")
                .into_response());
        }
        Ok(endpoint_auth(ep).call(req).await?.into_response())
    })
}

#[OpenApi]
impl Api {
    #[oai(
        path = "/profile/credentials",
        method = "get",
        operation_id = "get_my_credentials",
        transform = "parameters_based_auth"
    )]
    async fn api_get_credentials_state(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<CredentialsStateResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(CredentialsStateResponse::Unauthorized);
        };

        let user_cfg = User::try_from(user.clone())?;

        let otp_creds = user
            .find_related(entities::OtpCredential::Entity)
            .all(&*db)
            .await?;
        let password_creds = user
            .find_related(entities::PasswordCredential::Entity)
            .all(&*db)
            .await?;
        let sso_creds = user
            .find_related(entities::SsoCredential::Entity)
            .all(&*db)
            .await?;

        let pk_creds = user
            .find_related(entities::PublicKeyCredential::Entity)
            .all(&*db)
            .await?;

        let cert_creds = user
            .find_related(entities::CertificateCredential::Entity)
            .all(&*db)
            .await?;

        Ok(CredentialsStateResponse::Ok(Json(CredentialsState {
            password: match password_creds.len() {
                0 => PasswordState::Unset,
                1 => PasswordState::Set,
                _ => PasswordState::MultipleSet,
            },
            otp: otp_creds.into_iter().map(Into::into).collect(),
            public_keys: pk_creds.into_iter().map(Into::into).collect(),
            certificates: cert_creds.into_iter().map(Into::into).collect(),
            sso: sso_creds.into_iter().map(Into::into).collect(),
            credential_policy: user_cfg.credential_policy.unwrap_or_default(),
            ldap_linked: user.ldap_server_id.is_some(),
        })))
    }

    #[oai(
        path = "/profile/credentials/password",
        method = "post",
        operation_id = "change_my_password",
        transform = "parameters_based_auth"
    )]
    async fn api_change_password(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<ChangePasswordRequest>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<ChangePasswordResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(ChangePasswordResponse::Unauthorized);
        };

        entities::PasswordCredential::Entity::delete_many()
            .filter(entities::PasswordCredential::Column::UserId.eq(user.id))
            .exec(&*db)
            .await
            .map_err(WarpgateError::from)?;

        let new_credential = entities::PasswordCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user.id),
            ..PasswordCredential::ActiveModel::from(UserPasswordCredential::from_password(
                &body.password.clone().into(),
            ))
        }
        .insert(&*db)
        .await
        .map_err(WarpgateError::from)?;

        entities::PasswordCredential::Entity::find()
            .filter(
                entities::PasswordCredential::Column::UserId
                    .eq(user.id)
                    .and(entities::PasswordCredential::Column::Id.ne(new_credential.id)),
            )
            .all(&*db)
            .await?;

        AuditEvent::CredentialCreated {
            credential_type: "password".to_string(),
            credential_name: Some("password".to_string()),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(ChangePasswordResponse::Done(Json(PasswordState::Set)))
    }

    #[oai(
        path = "/profile/credentials/public-keys",
        method = "post",
        operation_id = "add_my_public_key",
        transform = "parameters_based_auth"
    )]
    async fn api_create_pk(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewPublicKeyCredential>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<CreatePublicKeyCredentialResponse, WarpgateError> {
        let Some(user) = current_user(&ctx).await? else {
            return Ok(CreatePublicKeyCredentialResponse::Unauthorized);
        };
        let target_id = validate_ssh_target(&ctx, &user.username, body.target_id).await?;
        let normalized_key = normalize_public_key(&body.openssh_public_key)?;

        let db = ctx.services().db.lock().await;
        let object = PublicKeyCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user.id),
            target_id: Set(target_id),
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
        .await
        .map_err(WarpgateError::from)?;

        let credential_name = body.label.clone();
        AuditEvent::CredentialCreated {
            credential_type: "public_key".to_string(),
            credential_name: Some(credential_name),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(CreatePublicKeyCredentialResponse::Created(Json(
            object.into(),
        )))
    }

    #[oai(
        path = "/profile/credentials/public-keys/issue",
        method = "post",
        operation_id = "issue_my_public_key",
        transform = "parameters_based_auth"
    )]
    async fn api_issue_pk(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<IssuePublicKeyCredentialRequest>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<IssuePublicKeyCredentialResponse, WarpgateError> {
        let Some(user) = current_user(&ctx).await? else {
            return Ok(IssuePublicKeyCredentialResponse::Unauthorized);
        };

        let max_uses = parse_positive_option(body.max_uses, "max_uses")?;
        let expires_at = compute_expiry(body.valid_for_seconds)?;
        let uses_left = max_uses;
        let target_id = validate_ssh_target(&ctx, &user.username, Some(body.target_id)).await?;
        let algorithm = body.algorithm.unwrap_or(IssuedPublicKeyAlgorithm::Ed25519);
        let (openssh_public_key, private_key_openssh) = generate_issued_keypair(algorithm)?;

        let db = ctx.services().db.lock().await;
        let object = PublicKeyCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user.id),
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
        .await
        .map_err(WarpgateError::from)?;

        AuditEvent::CredentialCreated {
            credential_type: "issued_public_key".to_string(),
            credential_name: Some(body.label.clone()),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
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

    #[oai(
        path = "/profile/credentials/public-keys/:id",
        method = "delete",
        operation_id = "delete_my_public_key",
        transform = "parameters_based_auth"
    )]
    async fn api_delete_pk(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<DeleteCredentialResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(DeleteCredentialResponse::Unauthorized);
        };

        let Some(model) = user
            .find_related(entities::PublicKeyCredential::Entity)
            .filter(entities::PublicKeyCredential::Column::Id.eq(id.0))
            .one(&*db)
            .await?
        else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        model.delete(&*db).await?;

        AuditEvent::CredentialDeleted {
            credential_type: "public_key".to_string(),
            credential_name: Some("public_key".to_string()),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(DeleteCredentialResponse::Deleted)
    }

    #[oai(
        path = "/profile/credentials/public-keys/:id/revoke",
        method = "post",
        operation_id = "revoke_my_public_key",
        transform = "parameters_based_auth"
    )]
    async fn api_revoke_pk(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<RevokeCredentialResponse, WarpgateError> {
        let Some(user) = current_user(&ctx).await? else {
            return Ok(RevokeCredentialResponse::Unauthorized);
        };

        let db = ctx.services().db.lock().await;
        let Some(existing) = user
            .find_related(entities::PublicKeyCredential::Entity)
            .filter(entities::PublicKeyCredential::Column::Id.eq(id.0))
            .filter(entities::PublicKeyCredential::Column::IssuedByWarpgate.eq(true))
            .one(&*db)
            .await?
        else {
            return Ok(RevokeCredentialResponse::NotFound);
        };

        if existing.revoked_at.is_none() {
            let credential_name = existing.label.clone();
            let mut model: entities::PublicKeyCredential::ActiveModel = existing.into();
            model.revoked_at = Set(Some(OffsetDateTime::now_utc()));
            model.uses_left = Set(Some(0));
            model.update(&*db).await?;

            AuditEvent::CredentialDeleted {
                credential_type: "issued_public_key".to_string(),
                credential_name: Some(credential_name),
                via: CredentialChangedVia::SelfService,
                user_id: user.id,
                username: user.username.clone(),
                actor_user_id: ctx.auth.user_id(),
            }
            .emit();
        }

        Ok(RevokeCredentialResponse::Revoked)
    }

    #[oai(
        path = "/profile/credentials/otp",
        method = "post",
        operation_id = "add_my_otp",
        transform = "parameters_based_auth"
    )]
    async fn api_create_otp(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewOtpCredential>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<CreateOtpCredentialResponse, WarpgateError> {
        let Some(user) = current_user(&ctx).await? else {
            return Ok(CreateOtpCredentialResponse::Unauthorized);
        };

        let user_id = user.id;
        let username = user.username.clone();
        let mut user_cfg: User = user.clone().try_into()?;
        let target_id = validate_ssh_target(&ctx, &username, body.target_id).await?;

        let db = ctx.services().db.lock().await;
        let object = entities::OtpCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            target_id: Set(target_id),
            secret_key: Set(body.secret_key.clone()),
        }
        .insert(&*db)
        .await
        .map_err(WarpgateError::from)?;

        let details = user.load_details(&db).await?;
        user_cfg.credential_policy = Some(
            user_cfg
                .credential_policy
                .unwrap_or_default()
                .upgrade_to_otp(details.credentials.as_slice()),
        );

        let user = entities::User::ActiveModel::try_from(user_cfg)?;
        user.update(&*db).await?;

        AuditEvent::CredentialCreated {
            credential_type: "otp".to_string(),
            credential_name: Some("otp".to_string()),
            via: CredentialChangedVia::SelfService,
            user_id,
            username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(CreateOtpCredentialResponse::Created(Json(object.into())))
    }

    #[oai(
        path = "/profile/credentials/otp/:id",
        method = "delete",
        operation_id = "delete_my_otp",
        transform = "parameters_based_auth"
    )]
    async fn api_delete_otp(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<DeleteCredentialResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(DeleteCredentialResponse::Unauthorized);
        };

        let Some(model) = user
            .find_related(entities::OtpCredential::Entity)
            .filter(entities::OtpCredential::Column::Id.eq(id.0))
            .one(&*db)
            .await?
        else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        model.delete(&*db).await?;

        AuditEvent::CredentialDeleted {
            credential_type: "otp".to_string(),
            credential_name: Some("otp".to_string()),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(DeleteCredentialResponse::Deleted)
    }

    #[oai(
        path = "/profile/credentials/certificates",
        method = "post",
        operation_id = "issue_my_certificate",
        transform = "parameters_based_auth"
    )]
    async fn api_issue_certificate(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<IssueCertificateCredentialRequest>,
    ) -> Result<IssueCertificateCredentialResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(IssueCertificateCredentialResponse::Unauthorized);
        };

        // Fetch CA params
        let params = Parameters::Entity::get(&db).await?;
        let ca =
            warpgate_ca::deserialize_ca(&params.ca_certificate_pem, &params.ca_private_key_pem)?;
        let public_key_pem = body.public_key_pem.trim();
        let client_cert =
            warpgate_ca::issue_client_certificate(&ca, &user.username, public_key_pem, user.id)?;
        let client_cert_pem = warpgate_ca::certificate_to_pem(&client_cert)?;

        let object = CertificateCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user.id),
            date_added: Set(Some(OffsetDateTime::now_utc())),
            last_used: Set(None),
            label: Set(body.label.clone()),
            certificate_pem: Set(client_cert_pem.clone()),
        }
        .insert(&*db)
        .await
        .map_err(WarpgateError::from)?;

        let credential_name = body.label.clone();
        AuditEvent::CredentialCreated {
            credential_type: "certificate".to_string(),
            credential_name: Some(credential_name),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(IssueCertificateCredentialResponse::Issued(Json(
            IssuedCertificateCredential {
                credential: object.into(),
                certificate_pem: client_cert_pem,
            },
        )))
    }

    #[oai(
        path = "/profile/credentials/certificates/:id",
        method = "delete",
        operation_id = "revoke_my_certificate",
        transform = "parameters_based_auth"
    )]
    async fn api_revoke_certificate(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        id: Path<Uuid>,
    ) -> Result<DeleteCertificateCredentialResponse, WarpgateError> {
        let auth = &ctx.auth;
        let db = ctx.services().db.lock().await;

        let Some(user) = get_user(auth, &db).await? else {
            return Ok(DeleteCertificateCredentialResponse::Unauthorized);
        };

        let Some(model) = user
            .find_related(entities::CertificateCredential::Entity)
            .filter(entities::CertificateCredential::Column::Id.eq(id.0))
            .one(&*db)
            .await?
        else {
            return Ok(DeleteCertificateCredentialResponse::NotFound);
        };

        // Add to revocation list
        let cert = warpgate_ca::deserialize_certificate(&model.certificate_pem)?;
        entities::CertificateRevocation::ActiveModel {
            id: Set(Uuid::new_v4()),
            date_added: Set(OffsetDateTime::now_utc()),
            serial_number_base64: Set(warpgate_ca::serialize_certificate_serial(&cert)),
        }
        .insert(&*db)
        .await?;

        model.delete(&*db).await?;

        AuditEvent::CredentialDeleted {
            credential_type: "certificate".to_string(),
            credential_name: Some("certificate".to_string()),
            via: CredentialChangedVia::SelfService,
            user_id: user.id,
            username: user.username.clone(),
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(DeleteCertificateCredentialResponse::Ok)
    }
}
