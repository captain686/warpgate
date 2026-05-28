use std::time::Duration;

use http::StatusCode;
use poem::{Error as PoemError, IntoResponse};
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
pub enum HttpBoundaryError {
    #[error("invalid upstream URI for target {target}: {source}")]
    InvalidTargetUri {
        target: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("upstream operation timed out during {phase} for target {target} after {timeout:?}")]
    UpstreamTimeout {
        target: String,
        phase: &'static str,
        timeout: Duration,
    },

    #[error("upstream operation failed during {phase} for target {target}: {source}")]
    UpstreamFailure {
        target: String,
        phase: &'static str,
        #[source]
        source: anyhow::Error,
    },

    #[error("gateway internal failure during {phase}: {source}")]
    Internal {
        phase: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

#[derive(Clone, Copy)]
struct PublicHttpError {
    status: StatusCode,
    title: &'static str,
    message: &'static str,
}

impl HttpBoundaryError {
    fn public_error(&self) -> PublicHttpError {
        match self {
            Self::UpstreamTimeout { .. } => PublicHttpError {
                status: StatusCode::GATEWAY_TIMEOUT,
                title: "Gateway timeout",
                message: "The upstream service did not respond in time.",
            },
            Self::InvalidTargetUri { .. }
            | Self::UpstreamFailure { .. }
            | Self::Internal { .. } => PublicHttpError {
                status: StatusCode::BAD_GATEWAY,
                title: "Gateway error",
                message: "The upstream service could not be reached.",
            },
        }
    }

    pub fn into_public_poem_error(self) -> PoemError {
        let public = self.public_error();
        error!(
            internal_error=?self,
            public_status=%public.status,
            public_message=public.message,
            "HTTP boundary failure"
        );
        PoemError::from_string(public.message, public.status)
    }
}

fn public_error_from_status(status: StatusCode) -> PublicHttpError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => PublicHttpError {
            status,
            title: "Access denied",
            message: "Access denied.",
        },
        StatusCode::GATEWAY_TIMEOUT => PublicHttpError {
            status,
            title: "Gateway timeout",
            message: "The upstream service did not respond in time.",
        },
        _ if status.is_client_error() => PublicHttpError {
            status,
            title: "Invalid request",
            message: "The request could not be processed.",
        },
        _ => PublicHttpError {
            status: StatusCode::BAD_GATEWAY,
            title: "Gateway error",
            message: "The upstream service could not be reached.",
        },
    }
}

pub fn error_page(e: &poem::Error) -> impl IntoResponse {
    let public = public_error_from_status(e.status());
    error!(
        internal_error=?e,
        public_status=%public.status,
        public_message=public.message,
        "HTTP page request failed"
    );
    poem::web::Html(format!(
        r#"<!DOCTYPE html>
        <style>
            body {{
                font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif, "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol";
            }}

            img {{
                width: 100px;
            }}

            main {{
                width: 400px;
                margin: 200px auto;
            }}
        </style>
        <main>
            <img src="/@warpgate/assets/brand.svg" />
            <h1>{}</h1>
            <p>{}</p>
        </main>
        "#,
        public.title,
        public.message,
    )).with_status(public.status)
}
