use std::time::Duration;

use poem::Request;
use poem::session::Session;
use poem::web::Data;
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, Object, OpenApi};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use tracing::{debug, warn};
use warpgate_common::WarpgateError;
use warpgate_common_http::auth::UnauthenticatedRequestContext;
use warpgate_common_http::ext::construct_external_url;
use warpgate_sso::{SsoClient, SsoLoginRequest};

pub struct Api;
const SSO_PROVIDER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Object)]
struct StartSsoResponseParams {
    url: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(ApiResponse)]
enum StartSsoResponse {
    #[oai(status = 200)]
    Ok(Json<StartSsoResponseParams>),
    #[oai(status = 404)]
    NotFound,
    #[oai(status = 503)]
    ProviderUnavailable,
    #[oai(status = 504)]
    ProviderTimeout,
}

pub static SSO_CONTEXT_SESSION_KEY: &str = "sso_request";

#[derive(Debug, Serialize, Deserialize)]
pub struct SsoContext {
    pub provider: String,
    pub request: SsoLoginRequest,
    pub next_url: Option<String>,
    pub supports_single_logout: bool,
    pub return_host: Option<String>,
}

#[OpenApi]
impl Api {
    #[oai(
        path = "/sso/providers/:name/start",
        method = "get",
        operation_id = "start_sso"
    )]
    async fn api_start_sso(
        &self,
        req: &Request,
        session: &Session,
        ctx: Data<&UnauthenticatedRequestContext>,
        name: Path<String>,
        next: Query<Option<String>>,
    ) -> Result<StartSsoResponse, WarpgateError> {
        let config = ctx.services().config.lock().await;

        let name = name.0;

        let Some(provider_config) = config.store.sso_providers.iter().find(|p| p.name == *name)
        else {
            return Ok(StartSsoResponse::NotFound);
        };
        let mut return_url = construct_external_url(
            Some(req),
            &config,
            provider_config.return_domain_whitelist.as_deref(),
        )
        .await?;
        return_url.set_path(&format!(
            "{}warpgate/api/sso/return",
            provider_config.return_url_prefix
        ));
        debug!("Return URL: {return_url}");

        let client = match SsoClient::new(provider_config.provider.clone()) {
            Ok(client) => client,
            Err(error) => {
                warn!(provider=%name, ?error, "Failed to initialize SSO provider");
                return Ok(StartSsoResponse::ProviderUnavailable);
            }
        };

        let sso_req = match timeout(
            SSO_PROVIDER_TIMEOUT,
            client.start_login(return_url.to_string()),
        )
        .await
        {
            Ok(Ok(request)) => request,
            Ok(Err(error)) => {
                warn!(provider=%name, ?error, "SSO provider failed to start login");
                return Ok(StartSsoResponse::ProviderUnavailable);
            }
            Err(_) => {
                warn!(provider=%name, timeout=?SSO_PROVIDER_TIMEOUT, "SSO provider start login timed out");
                return Ok(StartSsoResponse::ProviderTimeout);
            }
        };
        let return_host = ctx.trusted_host_header(req);

        let url = sso_req.auth_url().to_string();
        let supports_single_logout = match timeout(
            SSO_PROVIDER_TIMEOUT,
            client.supports_single_logout(),
        )
        .await
        {
            Ok(Ok(supports_single_logout)) => supports_single_logout,
            Ok(Err(error)) => {
                warn!(provider=%name, ?error, "SSO provider capability lookup failed");
                return Ok(StartSsoResponse::ProviderUnavailable);
            }
            Err(_) => {
                warn!(provider=%name, timeout=?SSO_PROVIDER_TIMEOUT, "SSO provider capability lookup timed out");
                return Ok(StartSsoResponse::ProviderTimeout);
            }
        };
        session.set(
            SSO_CONTEXT_SESSION_KEY,
            SsoContext {
                provider: name,
                request: sso_req,
                next_url: next.0.clone(),
                supports_single_logout,
                return_host,
            },
        );

        Ok(StartSsoResponse::Ok(Json(StartSsoResponseParams { url })))
    }
}
