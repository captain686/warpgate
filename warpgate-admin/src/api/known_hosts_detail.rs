use poem::web::Data;
use poem_openapi::param::Path;
use poem_openapi::{ApiResponse, OpenApi};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;
use warpgate_common::{AdminPermission, WarpgateError};
use warpgate_common_http::AuthenticatedRequestContext;
use warpgate_db_entities::KnownHost;

use super::AnySecurityScheme;
use crate::api::common::require_admin_permission;
pub struct Api;

#[derive(ApiResponse)]
enum DeleteSSHKnownHostResponse {
    #[oai(status = 204)]
    Deleted,

    #[oai(status = 404)]
    NotFound,
}

#[OpenApi]
impl Api {
    #[oai(
        path = "/ssh/known-hosts/:id",
        method = "delete",
        operation_id = "delete_ssh_known_host"
    )]
    async fn api_ssh_delete_known_host(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        id: Path<Uuid>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<DeleteSSHKnownHostResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::ConfigEdit)).await?;

        let db = ctx.services().db.lock().await;

        let known_host = KnownHost::Entity::find_by_id(id.0).one(&*db).await?;

        match known_host {
            Some(known_host) => {
                KnownHost::Entity::delete_many()
                    .filter(KnownHost::Column::Host.eq(&known_host.host))
                    .filter(KnownHost::Column::Port.eq(known_host.port))
                    .filter(KnownHost::Column::KeyType.eq(&known_host.key_type))
                    .filter(KnownHost::Column::KeyBase64.eq(&known_host.key_base64))
                    .exec(&*db)
                    .await?;
                Ok(DeleteSSHKnownHostResponse::Deleted)
            }
            None => Ok(DeleteSSHKnownHostResponse::NotFound),
        }
    }
}
