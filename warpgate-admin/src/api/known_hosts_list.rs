use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Context;
use poem::web::Data;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Object, OpenApi};
use russh::keys::{Algorithm, PublicKey};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;
use warpgate_common::{AdminPermission, WarpgateError};
use warpgate_common_http::AuthenticatedRequestContext;
use warpgate_db_entities::KnownHost;

use super::AnySecurityScheme;
use crate::api::common::require_admin_permission;

pub struct Api;

fn dedupe_known_hosts(hosts: Vec<KnownHost::Model>) -> Vec<KnownHost::Model> {
    let mut seen = HashSet::new();
    hosts
        .into_iter()
        .filter(|host| {
            seen.insert((
                host.host.clone(),
                host.port,
                host.key_type.clone(),
                host.key_base64.clone(),
            ))
        })
        .collect()
}

#[derive(ApiResponse)]
enum GetSSHKnownHostsResponse {
    #[oai(status = 200)]
    Ok(Json<Vec<KnownHost::Model>>),
}

#[derive(ApiResponse)]
enum AddSshKnownHostResponse {
    #[oai(status = 200)]
    Ok(Json<KnownHost::Model>),
}

#[derive(Object)]
struct AddSshKnownHostRequest {
    host: String,
    port: i32,
    key_type: String,
    key_base64: String,
}

#[OpenApi]
impl Api {
    #[oai(
        path = "/ssh/known-hosts",
        method = "post",
        operation_id = "add_ssh_known_host"
    )]
    async fn add_ssh_known_host(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        body: Json<AddSshKnownHostRequest>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<AddSshKnownHostResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::ConfigEdit)).await?;

        // Validate
        Algorithm::from_str(&body.key_type).context("parsing key type")?;
        PublicKey::from_openssh(&format!("{} {}", body.key_type, body.key_base64))
            .context("parsing key")?;

        let db = ctx.services().db.lock().await;
        if let Some(model) = KnownHost::Entity::find()
            .filter(KnownHost::Column::Host.eq(&body.host))
            .filter(KnownHost::Column::Port.eq(body.port))
            .filter(KnownHost::Column::KeyType.eq(&body.key_type))
            .filter(KnownHost::Column::KeyBase64.eq(&body.key_base64))
            .one(&*db)
            .await?
        {
            return Ok(AddSshKnownHostResponse::Ok(Json(model)));
        }

        let model = KnownHost::ActiveModel {
            id: Set(Uuid::new_v4()),
            host: Set(body.host.clone()),
            port: Set(body.port),
            key_type: Set(body.key_type.clone()),
            key_base64: Set(body.key_base64.clone()),
        }
        .insert(&*db)
        .await?;
        Ok(AddSshKnownHostResponse::Ok(Json(model)))
    }

    #[oai(
        path = "/ssh/known-hosts",
        method = "get",
        operation_id = "get_ssh_known_hosts"
    )]
    async fn get_ssh_known_hosts(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        _sec_scheme: AnySecurityScheme,
    ) -> Result<GetSSHKnownHostsResponse, WarpgateError> {
        require_admin_permission(&ctx, Some(AdminPermission::ConfigEdit)).await?;

        let db = ctx.services().db.lock().await;
        let hosts = KnownHost::Entity::find()
            .order_by_asc(KnownHost::Column::Host)
            .order_by_asc(KnownHost::Column::Port)
            .order_by_asc(KnownHost::Column::KeyType)
            .order_by_asc(KnownHost::Column::KeyBase64)
            .order_by_asc(KnownHost::Column::Id)
            .all(&*db)
            .await?;
        Ok(GetSSHKnownHostsResponse::Ok(Json(dedupe_known_hosts(
            hosts,
        ))))
    }
}
