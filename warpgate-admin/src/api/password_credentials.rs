use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Object, OpenApi};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, Set,
};
use uuid::Uuid;
use warpgate_common::{AdminPermission, Secret, UserPasswordCredential, WarpgateError};
use warpgate_common_http::AuthenticatedRequestContext;
use warpgate_core::logging::{AuditEvent, CredentialChangedVia};
use warpgate_db_entities::{PasswordCredential, User};

use super::AnySecurityScheme;
use crate::api::common::{require_admin_permission, require_manage_admin_accounts_permission};

#[derive(Object)]
struct ExistingPasswordCredential {
    id: Uuid,
}

#[derive(Object)]
struct NewPasswordCredential {
    password: Secret<String>,
}

impl From<PasswordCredential::Model> for ExistingPasswordCredential {
    fn from(credential: PasswordCredential::Model) -> Self {
        Self { id: credential.id }
    }
}

#[derive(ApiResponse)]
enum GetPasswordCredentialsResponse {
    #[oai(status = 200)]
    Ok(Json<Vec<ExistingPasswordCredential>>),
}

#[derive(ApiResponse)]
enum CreatePasswordCredentialResponse {
    #[oai(status = 201)]
    Created(Json<ExistingPasswordCredential>),
    #[oai(status = 400)]
    BadRequest(Json<String>),
    #[oai(status = 404)]
    NotFound,
}

#[derive(ApiResponse)]
enum UpdatePasswordCredentialResponse {
    #[oai(status = 200)]
    Updated(Json<ExistingPasswordCredential>),
    #[oai(status = 400)]
    BadRequest(Json<String>),
    #[oai(status = 404)]
    NotFound,
}

fn validate_password(password: &Secret<String>) -> Result<(), String> {
    if password.expose_secret().trim().is_empty() {
        return Err("password".into());
    }
    Ok(())
}

pub struct ListApi;

#[OpenApi]
impl ListApi {
    #[oai(
        path = "/users/:user_id/credentials/passwords",
        method = "get",
        operation_id = "get_password_credentials"
    )]
    async fn api_get_all(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        user_id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<GetPasswordCredentialsResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        let db = ctx.services().db.lock().await;

        let objects = PasswordCredential::Entity::find()
            .filter(PasswordCredential::Column::UserId.eq(*user_id))
            .all(&*db)
            .await?;

        Ok(GetPasswordCredentialsResponse::Ok(Json(
            objects.into_iter().map(Into::into).collect(),
        )))
    }

    #[oai(
        path = "/users/:user_id/credentials/passwords",
        method = "post",
        operation_id = "create_password_credential"
    )]
    async fn api_create(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewPasswordCredential>,
        user_id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<CreatePasswordCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        if let Err(field) = validate_password(&body.password) {
            return Ok(CreatePasswordCredentialResponse::BadRequest(Json(field)));
        }

        let db = ctx.services().db.lock().await;

        let object = PasswordCredential::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(*user_id),
            ..PasswordCredential::ActiveModel::from(UserPasswordCredential::from_password(
                &body.password,
            ))
        }
        .insert(&*db)
        .await
        .map_err(WarpgateError::from)?;

        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(CreatePasswordCredentialResponse::NotFound);
        };

        AuditEvent::CredentialCreated {
            credential_type: "password".to_string(),
            credential_name: None,
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(CreatePasswordCredentialResponse::Created(Json(
            object.into(),
        )))
    }
}

#[derive(ApiResponse)]
enum DeleteCredentialResponse {
    #[oai(status = 204)]
    Deleted,
    #[oai(status = 403)]
    Forbidden(Json<String>),
    #[oai(status = 404)]
    NotFound,
}

pub struct DetailApi;

#[OpenApi]
impl DetailApi {
    #[oai(
        path = "/users/:user_id/credentials/passwords/:id",
        method = "put",
        operation_id = "update_password_credential"
    )]
    async fn api_update(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<NewPasswordCredential>,
        user_id: Path<Uuid>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<UpdatePasswordCredentialResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::UsersEdit)).await?;
        require_manage_admin_accounts_permission(&ctx, *user_id).await?;

        if let Err(field) = validate_password(&body.password) {
            return Ok(UpdatePasswordCredentialResponse::BadRequest(Json(field)));
        }

        let db = ctx.services().db.lock().await;

        let Some(existing) = PasswordCredential::Entity::find_by_id(id.0)
            .filter(PasswordCredential::Column::UserId.eq(*user_id))
            .one(&*db)
            .await?
        else {
            return Ok(UpdatePasswordCredentialResponse::NotFound);
        };

        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(UpdatePasswordCredentialResponse::NotFound);
        };

        let mut model: PasswordCredential::ActiveModel = existing.into();
        let new_model = PasswordCredential::ActiveModel::from(UserPasswordCredential::from_password(
            &body.password,
        ));
        model.argon_hash = new_model.argon_hash;
        let updated = model.update(&*db).await?;

        AuditEvent::CredentialUpdated {
            credential_type: "password".to_string(),
            credential_name: None,
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(UpdatePasswordCredentialResponse::Updated(Json(updated.into())))
    }

    #[oai(
        path = "/users/:user_id/credentials/passwords/:id",
        method = "delete",
        operation_id = "delete_password_credential"
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

        let Some(model) = PasswordCredential::Entity::find_by_id(id.0)
            .filter(PasswordCredential::Column::UserId.eq(*user_id))
            .one(&*db)
            .await?
        else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        let password_count = PasswordCredential::Entity::find()
            .filter(PasswordCredential::Column::UserId.eq(*user_id))
            .count(&*db)
            .await?;
        if password_count <= 1 {
            return Ok(DeleteCredentialResponse::Forbidden(Json(
                "Each user must keep at least one password credential.".to_string(),
            )));
        }

        model.delete(&*db).await?;

        let Some(user) = User::Entity::find_by_id(*user_id).one(&*db).await? else {
            return Ok(DeleteCredentialResponse::NotFound);
        };

        AuditEvent::CredentialDeleted {
            credential_type: "password".to_string(),
            credential_name: None,
            via: CredentialChangedVia::Admin,
            user_id: *user_id,
            username: user.username,
            actor_user_id: ctx.auth.user_id(),
        }
        .emit();

        Ok(DeleteCredentialResponse::Deleted)
    }
}
