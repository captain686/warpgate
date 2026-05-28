use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use cookie::Cookie;
use data_encoding::BASE64;
use delegate::delegate;
use futures::{StreamExt, TryStreamExt};
use http::header::HeaderName;
use http::uri::{Authority, Scheme};
use http::{HeaderValue, Uri};
use poem::session::Session;
use poem::web::websocket::WebSocket;
use poem::{Body, FromRequest, IntoResponse, Request, Response};
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite};
use tracing::{debug, error, warn};
use url::Url;
use warpgate_common::helpers::websocket::pump_websocket;
use warpgate_common::http_headers::{
    DONT_FORWARD_HEADERS, X_FORWARDED_FOR, X_FORWARDED_HOST, X_FORWARDED_PROTO,
};
use warpgate_common::{TargetHTTPOptions, WarpgateError, try_block};
use warpgate_common_http::logging::{get_client_ip, log_request_result};
use warpgate_common_http::{AuthenticatedRequestContext, SessionAuthorization};
use warpgate_tls::{TlsMode, configure_tls_connector};
use warpgate_web::lookup_built_file;

use crate::common::SessionExt;
use crate::error::HttpBoundaryError;

static X_WARPGATE_USERNAME: HeaderName = HeaderName::from_static("x-warpgate-username");
static X_WARPGATE_AUTHENTICATION_TYPE: HeaderName =
    HeaderName::from_static("x-warpgate-authentication-type");
const HTTP_TARGET_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_TARGET_READ_TIMEOUT: Duration = Duration::from_secs(60);
const HTTP_TARGET_WEBSOCKET_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

trait SomeResponse {
    fn status(&self) -> http::StatusCode;
    fn headers(&self) -> &http::HeaderMap;
}

impl SomeResponse for reqwest::Response {
    delegate! {
        to self {
            fn status(&self) -> http::StatusCode;
            fn headers(&self) -> &http::HeaderMap;
        }
    }
}

impl<B> SomeResponse for http::Response<B> {
    delegate! {
        to self {
            fn status(&self) -> http::StatusCode;
            fn headers(&self) -> &http::HeaderMap;
        }
    }
}

trait SomeRequestBuilder {
    fn header<K: Into<HeaderName>, V>(self, k: K, v: V) -> Self
    where
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>;
}

impl SomeRequestBuilder for reqwest::RequestBuilder {
    fn header<K: Into<HeaderName>, V>(self, k: K, v: V) -> Self
    where
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.header(k, v)
    }
}

impl SomeRequestBuilder for http::request::Builder {
    fn header<K: Into<HeaderName>, V>(self, k: K, v: V) -> Self
    where
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.header(k, v)
    }
}

fn construct_uri(req: &Request, options: &TargetHTTPOptions, websocket: bool) -> Result<Uri> {
    let target_uri = Uri::try_from(options.url.clone())?;
    let source_uri = req.uri().clone();

    let authority = target_uri
        .authority()
        .context("No authority in the URL")?
        .to_string();

    let authority: Authority = authority.try_into()?;
    let mut uri = http::uri::Builder::new()
        .authority(authority)
        .path_and_query(
            source_uri
                .path_and_query()
                .context("No path in the URL")?
                .clone(),
        );

    let scheme = match options.tls.mode {
        TlsMode::Disabled => &Scheme::HTTP,
        TlsMode::Preferred => target_uri.scheme().context("No scheme in the URL")?,
        TlsMode::Required => &Scheme::HTTPS,
    };
    uri = uri.scheme(scheme.clone());

    if websocket {
        uri = uri.scheme(Scheme::from_str(if scheme == &Scheme::HTTP {
            "ws"
        } else {
            "wss"
        })?);
    }

    Ok(uri.build()?)
}

fn target_log_label(uri: &Uri) -> String {
    match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => format!("{scheme}://{authority}"),
        (Some(scheme), None) => scheme.to_string(),
        (None, Some(authority)) => authority.to_string(),
        (None, None) => "<unknown-target>".to_string(),
    }
}

fn configured_target_label(options: &TargetHTTPOptions) -> String {
    let Ok(mut url) = Url::parse(&options.url) else {
        return "<invalid-target-url>".to_string();
    };

    let _ = url.set_username("");
    let _ = url.set_password(None);

    match (url.scheme(), url.host_str(), url.port()) {
        (scheme, Some(host), Some(port)) => format!("{scheme}://{host}:{port}"),
        (scheme, Some(host), None) => format!("{scheme}://{host}"),
        (scheme, None, _) => scheme.to_string(),
    }
}

fn copy_client_response<R: SomeResponse>(
    client_response: &R,
    server_response: &mut poem::Response,
) {
    let mut headers = client_response.headers().clone();
    for h in client_response.headers() {
        if DONT_FORWARD_HEADERS.contains(h.0)
            && let http::header::Entry::Occupied(e) = headers.entry(h.0)
        {
            e.remove_entry();
        }
    }
    server_response.headers_mut().extend(headers);

    server_response.set_status(client_response.status());
}

fn rewrite_request<B: SomeRequestBuilder>(mut req: B, options: &TargetHTTPOptions) -> Result<B> {
    if let Some(ref headers) = options.headers {
        for (k, v) in headers {
            req = req.header(HeaderName::try_from(k)?, v);
        }
    }
    Ok(req)
}

fn rewrite_response(
    resp: &mut Response,
    options: &TargetHTTPOptions,
    source_uri: &Uri,
) -> Result<()> {
    let target_uri = Uri::try_from(options.url.clone())?;
    let headers = resp.headers_mut();

    if let Some(value) = headers.get_mut(http::header::LOCATION) {
        let location = Url::parse(&source_uri.to_string())?.join(value.to_str()?)?;
        let redirect_uri = Uri::try_from(location.to_string())?;

        if redirect_uri.authority() == target_uri.authority() {
            let old_value = value.clone();
            *value = Uri::builder()
                .path_and_query(
                    redirect_uri
                        .path_and_query()
                        .context("No path in URL")?
                        .clone(),
                )
                .build()?
                .to_string()
                .parse()?;
            debug!("Rewrote a redirect from {:?} to {:?}", old_value, value);
        }
    }

    if let http::header::Entry::Occupied(mut entry) = headers.entry(http::header::SET_COOKIE) {
        for value in entry.iter_mut() {
            try_block!({
                let mut cookie = Cookie::parse(value.to_str()?)?;
                cookie.set_expires(cookie::Expiration::Session);
                *value = cookie.to_string().parse()?;
            } catch (error: anyhow::Error) {
                warn!(?error, header=?value, "Failed to parse response cookie");
            });
        }
    }

    Ok(())
}

fn copy_server_request<B: SomeRequestBuilder>(req: &Request, mut target: B) -> B {
    for k in req.headers().keys() {
        if DONT_FORWARD_HEADERS.contains(k) {
            continue;
        }
        target = target.header(
            k.clone(),
            req.headers()
                .get_all(k)
                .iter()
                .map(|v| v.to_str().map(|x| x.to_string()))
                .filter_map(|x| x.ok())
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    target
}

fn inject_forwarding_headers<B: SomeRequestBuilder>(
    req: &Request,
    ctx: &AuthenticatedRequestContext,
    mut target: B,
) -> B {
    if let Some(host) = ctx.trusted_host_header(req) {
        target = target.header(X_FORWARDED_HOST.clone(), host);
    }
    target = target.header(X_FORWARDED_PROTO.clone(), ctx.trusted_proto(req).as_str());
    if let Some(addr) = req.remote_addr().as_socket_addr() {
        target = target.header(X_FORWARDED_FOR.clone(), addr.ip().to_string());
    }
    target
}

async fn inject_own_headers<B: SomeRequestBuilder>(req: &Request, mut target: B) -> Result<B> {
    let session = <&Session>::from_request_without_body(req).await?;
    if let Some(auth) = session.get_auth() {
        target = target.header(&X_WARPGATE_USERNAME, auth.username()).header(
            &X_WARPGATE_AUTHENTICATION_TYPE,
            match auth {
                SessionAuthorization::Ticket { .. } => "ticket",
                SessionAuthorization::User { .. } => "user",
            },
        );
    }
    Ok(target)
}

pub async fn proxy_normal_request(
    req: &Request,
    ctx: &AuthenticatedRequestContext,
    body: Body,
    options: &TargetHTTPOptions,
) -> poem::Result<Response> {
    let uri = construct_uri(req, options, false).map_err(|source| {
        HttpBoundaryError::InvalidTargetUri {
            target: configured_target_label(options),
            source,
        }
        .into_public_poem_error()
    })?;

    let (authorization_header, uri) = extract_basic_auth(uri).map_err(|source| {
        HttpBoundaryError::InvalidTargetUri {
            target: configured_target_label(options),
            source,
        }
        .into_public_poem_error()
    })?;
    let target = target_log_label(&uri);

    tracing::debug!(%target, "Proxying HTTP request");

    let mut client = reqwest::Client::builder()
        .gzip(true)
        .redirect(reqwest::redirect::Policy::none())
        .connection_verbose(true)
        .connect_timeout(HTTP_TARGET_CONNECT_TIMEOUT)
        .read_timeout(HTTP_TARGET_READ_TIMEOUT);

    if options.tls.mode == TlsMode::Required {
        client = client.https_only(true);
    }

    client = client.redirect(reqwest::redirect::Policy::custom({
        let tls_mode = options.tls.mode;
        let uri = uri.clone();
        move |attempt| {
            if tls_mode == TlsMode::Preferred
                && uri.scheme() == Some(&Scheme::HTTP)
                && attempt.url().scheme() == "https"
            {
                debug!("Following HTTP->HTTPS redirect");
                attempt.follow()
            } else {
                attempt.stop()
            }
        }
    }));

    if !options.tls.verify {
        client = client.danger_accept_invalid_certs(true);
    }

    let client = client.build().map_err(|source| {
        HttpBoundaryError::Internal {
            phase: "build HTTP client",
            source: source.into(),
        }
        .into_public_poem_error()
    })?;

    let mut client_request = client.request(req.method().into(), uri.to_string());

    client_request = copy_server_request(req, client_request);
    client_request = inject_forwarding_headers(req, ctx, client_request);
    client_request = inject_own_headers(req, client_request).await?;
    client_request = rewrite_request(client_request, options)?;
    if let Some(authorization_header) = authorization_header {
        client_request = client_request.header(http::header::AUTHORIZATION, authorization_header);
    }

    client_request = client_request.body(reqwest::Body::wrap_stream(body.into_bytes_stream()));

    let client_request = client_request.build().map_err(|source| {
        HttpBoundaryError::Internal {
            phase: "build HTTP request",
            source: source.into(),
        }
        .into_public_poem_error()
    })?;
    let client_response = client.execute(client_request).await.map_err(|source| {
        if source.is_timeout() {
            HttpBoundaryError::UpstreamTimeout {
                target: target.clone(),
                phase: "HTTP request",
                timeout: HTTP_TARGET_READ_TIMEOUT,
            }
        } else {
            HttpBoundaryError::UpstreamFailure {
                target: target.clone(),
                phase: "HTTP request",
                source: source.into(),
            }
        }
        .into_public_poem_error()
    })?;
    let status = client_response.status();

    let mut response: Response = "".into();

    copy_client_response(&client_response, &mut response);

    let embed_session_menu = {
        let db = ctx.services().db.lock().await;
        warpgate_db_entities::Parameters::Entity::get(&db)
            .await
            .map(|p| p.show_session_menu)
            .unwrap_or(true)
    };
    copy_client_body(client_response, &mut response, embed_session_menu)
        .await
        .map_err(|source| {
            HttpBoundaryError::UpstreamFailure {
                target: target.clone(),
                phase: "HTTP response body",
                source,
            }
            .into_public_poem_error()
        })?;

    log_request_result(
        req.method(),
        req.original_uri(),
        get_client_ip(req, ctx.services()).await.as_deref(),
        status,
    );

    rewrite_response(&mut response, options, &uri).map_err(|source| {
        HttpBoundaryError::UpstreamFailure {
            target,
            phase: "HTTP response rewrite",
            source,
        }
        .into_public_poem_error()
    })?;
    Ok(response)
}

async fn copy_client_body(
    client_response: reqwest::Response,
    response: &mut Response,
    embed_session_menu: bool,
) -> Result<()> {
    if embed_session_menu
        && response
            .content_type()
            .is_some_and(|c| c.starts_with("text/html"))
        && response.status() == 200
    {
        copy_client_body_and_embed(client_response, response).await?;
        return Ok(());
    }

    response.set_body(Body::from_bytes_stream(
        client_response
            .bytes_stream()
            .map_err(std::io::Error::other),
    ));
    Ok(())
}

async fn copy_client_body_and_embed(
    client_response: reqwest::Response,
    response: &mut Response,
) -> Result<()> {
    let content = client_response.text().await?;

    let script_manifest = lookup_built_file("src/embed/index.ts")?;

    let mut inject = format!(
        r#"<script type="module" src="/@warpgate/{}"></script>"#,
        script_manifest.file
    );
    for css_file in script_manifest.css.unwrap_or_default() {
        let _ = write!(
            &mut inject,
            r#"<link rel="stylesheet" href="/@warpgate/{css_file}" />"#
        );
    }

    let before = "</head>";
    let content = content.replacen(before, &format!("{inject}{before}"), 1);

    response.headers_mut().remove(http::header::CONTENT_LENGTH);
    response
        .headers_mut()
        .remove(http::header::CONTENT_ENCODING);
    response.headers_mut().remove(http::header::CONTENT_TYPE);
    response
        .headers_mut()
        .remove(http::header::TRANSFER_ENCODING);
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse()?,
    );
    response.set_body(content);
    Ok(())
}

pub async fn proxy_websocket_request(
    req: &Request,
    ws: WebSocket,
    ctx: &AuthenticatedRequestContext,
    options: &TargetHTTPOptions,
) -> poem::Result<impl IntoResponse> {
    let uri = construct_uri(req, options, true).map_err(|source| {
        HttpBoundaryError::InvalidTargetUri {
            target: configured_target_label(options),
            source,
        }
        .into_public_poem_error()
    })?;
    proxy_ws_inner(req, ws, uri.clone(), ctx, options)
        .await
        .map_err(|error| {
            tracing::error!(target=%target_log_label(&uri), ?error, "WebSocket proxy failed");
            error
        })
}

/// Remove the username/password from the URL before using it for the Host header
fn extract_basic_auth(uri: Uri) -> anyhow::Result<(Option<HeaderValue>, Uri)> {
    let uri_authority = uri
        .authority()
        .ok_or(WarpgateError::NoHostInUrl)?
        .to_string();
    let Some((creds, host)) = uri_authority.rsplit_once('@') else {
        return Ok((None, uri));
    };

    if host.is_empty() {
        anyhow::bail!("URL authority host is empty");
    }

    let uri = {
        let mut parts = uri.into_parts();
        parts.authority = Some(Authority::from_str(host)?);
        Uri::from_parts(parts)?
    };

    let auth_header = format!("Basic {}", BASE64.encode(creds.as_bytes()));

    let auth_value = HeaderValue::from_str(&auth_header)?;

    Ok((Some(auth_value), uri))
}

async fn proxy_ws_inner(
    req: &Request,
    ws: WebSocket,
    uri: Uri,
    ctx: &AuthenticatedRequestContext,
    options: &TargetHTTPOptions,
) -> poem::Result<impl IntoResponse> {
    let original_target = target_log_label(&uri);
    let (authorization_header, uri) = extract_basic_auth(uri).map_err(|source| {
        HttpBoundaryError::InvalidTargetUri {
            target: original_target,
            source,
        }
        .into_public_poem_error()
    })?;
    let target = target_log_label(&uri);
    let mut client_request = http::request::Builder::new()
        .uri(uri.clone())
        .header(http::header::CONNECTION, "Upgrade")
        .header(http::header::UPGRADE, "websocket")
        .header(http::header::SEC_WEBSOCKET_VERSION, "13")
        .header(
            http::header::SEC_WEBSOCKET_KEY,
            tungstenite::handshake::client::generate_key(),
        )
        // tungstenite requires an explicit Host header
        .header(
            http::header::HOST,
            uri.authority()
                .ok_or(WarpgateError::NoHostInUrl)
                .context("no authority in the URL")?
                .to_string(),
        );

    if let Some(authorization_header) = authorization_header {
        client_request = client_request.header(http::header::AUTHORIZATION, authorization_header);
    }

    client_request = copy_server_request(req, client_request);
    client_request = inject_forwarding_headers(req, ctx, client_request);
    client_request = inject_own_headers(req, client_request).await?;
    client_request = rewrite_request(client_request, options)?;

    let tls_config = configure_tls_connector(!options.tls.verify, false, None)
        .await
        .map_err(|source| {
            HttpBoundaryError::Internal {
                phase: "build WebSocket TLS connector",
                source: source.into(),
            }
            .into_public_poem_error()
        })?;
    let connector = Connector::Rustls(Arc::new(tls_config));

    let websocket_handshake = connect_async_tls_with_config(
        client_request.body(()).map_err(|source| {
            HttpBoundaryError::Internal {
                phase: "build WebSocket request",
                source: source.into(),
            }
            .into_public_poem_error()
        })?,
        None,
        true,
        Some(connector),
    );
    let (client, client_response) =
        match tokio::time::timeout(HTTP_TARGET_WEBSOCKET_HANDSHAKE_TIMEOUT, websocket_handshake)
            .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(source)) => {
                return Err(HttpBoundaryError::UpstreamFailure {
                    target,
                    phase: "WebSocket handshake",
                    source: source.into(),
                }
                .into_public_poem_error());
            }
            Err(_) => {
                return Err(HttpBoundaryError::UpstreamTimeout {
                    target,
                    phase: "WebSocket handshake",
                    timeout: HTTP_TARGET_WEBSOCKET_HANDSHAKE_TIMEOUT,
                }
                .into_public_poem_error());
            }
        };

    tracing::info!(target=%target_log_label(&uri), status=?client_response.status(), "WebSocket proxy connected");

    let mut response = ws
        .on_upgrade(|socket| async move {
            let (client_sink, client_source) = client.split();
            let (server_sink, server_source) = socket.split();

            if let Err(error) = {
                let server_to_client =
                    tokio::spawn(pump_websocket(server_source, client_sink, |msg| {
                        Box::pin(async {
                            tracing::debug!("Server: {:?}", msg);
                            anyhow::Ok(msg)
                        })
                    }));

                let client_to_server =
                    tokio::spawn(pump_websocket(client_source, server_sink, |msg| {
                        Box::pin(async {
                            tracing::debug!("Client: {:?}", msg);
                            anyhow::Ok(msg)
                        })
                    }));

                server_to_client.await??;
                client_to_server.await??;
                debug!("Closing Websocket stream");

                Ok::<_, anyhow::Error>(())
            } {
                error!(?error, "Websocket stream error");
            }
            Ok::<_, anyhow::Error>(())
        })
        .into_response();

    copy_client_response(&client_response, &mut response);
    rewrite_response(&mut response, options, &uri).map_err(|source| {
        HttpBoundaryError::UpstreamFailure {
            target: target_log_label(&uri),
            phase: "WebSocket response rewrite",
            source,
        }
        .into_public_poem_error()
    })?;
    Ok(response)
}
