use futures::{SinkExt, StreamExt};
use poem::session::Session;
use poem::web::Data;
use poem::web::websocket::{Message, WebSocket};
use poem::{IntoResponse, handler};
use poem_openapi::param::Query;
use poem_openapi::payload::Json;
use poem_openapi::{ApiResponse, OpenApi};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use tokio::sync::broadcast::error::RecvError;
use tracing::debug;
use warpgate_common::{AdminPermission, WarpgateError};
use warpgate_common_http::AuthenticatedRequestContext;
use warpgate_core::SessionSnapshot;

use super::AnySecurityScheme;
use super::pagination::{PaginatedResponse, PaginationParams};
use crate::api::common::require_admin_permission;

pub struct Api;

#[derive(ApiResponse)]
enum GetSessionsResponse {
    #[oai(status = 200)]
    Ok(Json<PaginatedResponse<SessionSnapshot>>),
}

#[derive(ApiResponse)]
enum CloseAllSessionsResponse {
    #[oai(status = 201)]
    Ok,
}

#[OpenApi]
impl Api {
    #[allow(clippy::too_many_arguments)]
    #[oai(path = "/sessions", method = "get", operation_id = "get_sessions")]
    #[allow(clippy::too_many_arguments)]
    async fn api_get_all_sessions(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        offset: Query<Option<u64>>,
        limit: Query<Option<u64>>,
        active_only: Query<Option<bool>>,
        logged_in_only: Query<Option<bool>>,
        username: Query<Option<String>>,
        _sec_scheme: AnySecurityScheme,
    ) -> poem::Result<GetSessionsResponse> {
        use warpgate_db_entities::Session;

        require_admin_permission(&ctx, Some(AdminPermission::SessionsView)).await?;

        let db = ctx.services().db.lock().await;
        let mut q = Session::Entity::find().order_by_desc(Session::Column::Started);

        if active_only.unwrap_or(false) {
            q = q.filter(Session::Column::Ended.is_null());
        }
        if logged_in_only.unwrap_or(false) {
            q = q.filter(Session::Column::Username.is_not_null());
        }
        if let Some(username_filter) = username.as_ref() {
            q = q.filter(Session::Column::Username.eq(username_filter.as_str()));
        }

        Ok(GetSessionsResponse::Ok(Json(
            PaginatedResponse::new(
                q,
                PaginationParams {
                    limit: *limit,
                    offset: *offset,
                },
                &*db,
                Into::into,
            )
            .await?,
        )))
    }

    #[oai(
        path = "/sessions",
        method = "delete",
        operation_id = "close_all_sessions"
    )]
    async fn api_close_all_sessions(
        &self,
        ctx: Data<&AuthenticatedRequestContext>,
        session: &Session,
        _sec_scheme: AnySecurityScheme,
    ) -> poem::Result<CloseAllSessionsResponse> {
        require_admin_permission(&ctx, Some(AdminPermission::SessionsTerminate)).await?;

        let mut state = ctx.services().state.lock().await;
        let session_ids = state.sessions.keys().copied().collect::<Vec<_>>();

        for id in &session_ids {
            if let Some(s) = state.sessions.get(id) {
                let mut session = s.lock().await;
                session.handle.close();
            }
        }

        for id in session_ids {
            state.remove_session(id).await;
        }

        session.purge();

        Ok(CloseAllSessionsResponse::Ok)
    }
}

#[handler]
pub async fn api_get_sessions_changes_stream(
    ctx: Data<&AuthenticatedRequestContext>,
    ws: WebSocket,
) -> Result<impl IntoResponse, WarpgateError> {
    require_admin_permission(&ctx, Some(AdminPermission::SessionsView)).await?;

    let mut receiver = ctx.services().state.lock().await.subscribe();

    Ok(ws
        .on_upgrade(|socket| async move {
            let (mut sink, _) = socket.split();

            loop {
                match receiver.recv().await {
                    Ok(()) => {
                        sink.send(Message::Text("".into())).await?;
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        debug!(
                            skipped,
                            "Session change stream lagged; sending latest-state notification"
                        );
                        sink.send(Message::Text("".into())).await?;
                    }
                    Err(RecvError::Closed) => break,
                }
            }

            Ok::<(), anyhow::Error>(())
        })
        .into_response())
}
