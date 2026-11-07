//! Shared transport for Basestation's BTS-initiated Telemetry/Control WebSocket
//! channels: single-step RFC 6455 upgrade at `/`, optional HTTP Basic auth,
//! and a required Sec-WebSocket-Protocol echo (the client aborts if we don't
//! echo back exactly what it offered).
use crate::config::TlsConfig;
use anyhow::Context;
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        ConnectInfo, FromRequestParts, State,
    },
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::Engine;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

#[derive(Clone)]
pub struct ListenerConfig {
    pub name: &'static str,
    pub listen: SocketAddr,
    pub subprotocol: &'static str,
    /// Username -> password, shared with the dashboard so logins added there
    /// apply to the next connection without a restart.
    pub users: crate::state::Users,
    pub tls: TlsConfig,
}

/// Identity of an authenticated connection: the Basic Auth username, or
/// `None` when the listener has no configured users (auth disabled).
pub type Identity = Option<String>;

#[derive(Clone)]
struct HandlerState<H: Clone> {
    cfg: Arc<ListenerConfig>,
    handler: H,
}

pub async fn serve<H, Fut>(cfg: ListenerConfig, handler: H) -> anyhow::Result<()>
where
    H: Fn(WebSocket, Identity, Option<SocketAddr>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let cfg = Arc::new(cfg);
    let state = HandlerState { cfg: cfg.clone(), handler };
    let app = Router::new()
        .route("/", get(upgrade_handler::<H, Fut>))
        .with_state(state);

    if cfg.tls.enabled {
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            &cfg.tls.cert_path,
            &cfg.tls.key_path,
        )
        .await
        .with_context(|| {
            format!(
                "loading TLS cert {} and key {} for {} listener",
                cfg.tls.cert_path.display(),
                cfg.tls.key_path.display(),
                cfg.name
            )
        })?;
        tracing::info!(listen=%cfg.listen, name=cfg.name, tls=true, "Basestation listener started");
        axum_server::bind_rustls(cfg.listen, rustls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(cfg.listen).await?;
        tracing::info!(listen=%cfg.listen, name=cfg.name, tls=false, "Basestation listener started");
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    }
    Ok(())
}

async fn upgrade_handler<H, Fut>(
    State(state): State<HandlerState<H>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<axum::body::Body>,
) -> Response
where
    H: Fn(WebSocket, Identity, Option<SocketAddr>) -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let peer = Some(peer);
    let (mut parts, _body) = request.into_parts();

    let verified = match state.cfg.users.read() {
        Ok(users) => verify_basic(&users, &parts.headers),
        Err(_) => Err(()),
    };
    let identity = match verified {
        Ok(identity) => identity,
        Err(()) => return basic_challenge(state.cfg.name),
    };

    let offered = parts
        .headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !offered.split(',').any(|p| p.trim() == state.cfg.subprotocol) {
        tracing::warn!(name = state.cfg.name, offered, expected = state.cfg.subprotocol, "subprotocol mismatch, rejecting");
        return (StatusCode::BAD_REQUEST, "missing or mismatched Sec-WebSocket-Protocol").into_response();
    }

    match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(ws) => {
            let handler = state.handler.clone();
            let subprotocol = state.cfg.subprotocol;
            ws.protocols([subprotocol])
                .on_upgrade(move |socket| handler(socket, identity, peer))
                .into_response()
        }
        Err(rejection) => rejection.into_response(),
    }
}

fn verify_basic(users: &HashMap<String, String>, headers: &HeaderMap) -> Result<Identity, ()> {
    if users.is_empty() {
        return Ok(None);
    }
    let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else { return Err(()) };
    let Some(b64) = value.strip_prefix("Basic ") else { return Err(()) };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(b64) else { return Err(()) };
    let Ok(text) = String::from_utf8(decoded) else { return Err(()) };
    let Some((user, pass)) = text.split_once(':') else { return Err(()) };
    if users.get(user).map(|p| p == pass).unwrap_or(false) {
        Ok(Some(user.to_string()))
    } else {
        Err(())
    }
}

fn basic_challenge(realm: &str) -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&format!("Basic realm=\"{realm}\"")).unwrap(),
    );
    response
}
