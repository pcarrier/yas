use axum::extract::ws::{Message, WebSocket};
use axum::extract::{FromRequest, State, WebSocketUpgrade};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use crate::transport::{self, read_frame, write_frame};

const WEB_INDEX_HTML_BR: &[u8] = include_bytes!("../../../js/ui/dist/index.html.br");
const WEB_SW_JS_BR: &[u8] = include_bytes!("../../../js/ui/dist/sw.js.br");
const YAS_SUBPROTOCOL: &str = yas_wire::schema::transport::WEBSOCKET_SUBPROTOCOL;
const YAS_PREFACE: &[u8; 8] = &yas_wire::PREFACE;
const YAS_MAX_FRAME_SIZE: usize = yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as usize;
const _: () = assert!(
    yas_wire::schema::transport::STREAM_LENGTH_BITS == u32::BITS as u8
        && yas_wire::schema::transport::STREAM_LENGTH_BYTES == size_of::<u32>()
);

struct BrowserState {
    auth_token: yas_webserver::config::AuthPassphrase,
    home_socket: String,
    home_server_uid: transport::HomeServerUid,
    shutdown: Arc<tokio::sync::Notify>,
    auth_throttle: yas_webserver::config::AuthThrottle,
}

/// Open the local browser UI backed by one native YAS server.
pub async fn run_browser(port: Option<u16>, _hub: &str) {
    let token: String = {
        use rand::RngExt as _;
        rand::rng()
            .sample_iter(rand::distr::Alphanumeric)
            .take(32)
            .map(|byte| byte as char)
            .collect()
    };
    let bind_port = port.unwrap_or(0);
    let home_socket = transport::default_local_socket();
    if let Err(error) = transport::ensure_local_server(&home_socket).await {
        eprintln!("yas: {error}");
        std::process::exit(1);
    }
    #[cfg(unix)]
    let home_server_uid = yas_webserver::local_ipc::expected_server_uid().unwrap_or_else(|error| {
        eprintln!("yas open: {error}");
        std::process::exit(1);
    });
    #[cfg(windows)]
    let home_server_uid = ();
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let state = Arc::new(BrowserState {
        auth_token: yas_webserver::config::AuthPassphrase::plaintext(token.clone()),
        home_socket,
        home_server_uid,
        shutdown: shutdown.clone(),
        auth_throttle: yas_webserver::config::AuthThrottle::new(),
    });
    let html_etag: &'static str =
        Box::leak(yas_webserver::html_etag(WEB_INDEX_HTML_BR).into_boxed_str());
    let sw_etag: &'static str = Box::leak(yas_webserver::html_etag(WEB_SW_JS_BR).into_boxed_str());
    let app = axum::Router::new()
        .fallback(get(move |state, request| {
            browser_root_handler(state, request, html_etag, sw_etag)
        }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{bind_port}"))
        .await
        .unwrap_or_else(|error| {
            eprintln!("yas: cannot bind to port {bind_port}: {error}");
            std::process::exit(1);
        });
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}/#psk={token}");
    eprintln!("yas: serving browser UI at {url}");
    open_browser(&url);

    let graceful = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown.notify_waiters();
    });
    if let Err(error) = graceful.await {
        eprintln!("yas: serve error: {error}");
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    eprintln!("yas: open {url} in your browser");
}

fn offers_yas_subprotocol(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get_all(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim() == YAS_SUBPROTOCOL)
}

async fn browser_root_handler(
    State(state): State<Arc<BrowserState>>,
    request: axum::extract::Request,
    etag: &'static str,
    sw_etag: &'static str,
) -> Response {
    let path = request.uri().path().to_string();
    let inm = request
        .headers()
        .get(axum::http::header::IF_NONE_MATCH)
        .map(|value| value.as_bytes());
    let ae = request
        .headers()
        .get(axum::http::header::ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok());
    if let Some(response) = yas_webserver::try_ui_route(&path, WEB_SW_JS_BR, sw_etag, inm, ae) {
        return response;
    }

    let is_ws = request
        .headers()
        .get(axum::http::header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if is_ws && path == "/edge" {
        if !offers_yas_subprotocol(request.headers()) {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "the /edge endpoint requires Sec-WebSocket-Protocol: yas.v1",
            )
                .into_response();
        }
        return match WebSocketUpgrade::from_request(request, &state).await {
            Ok(ws) => ws
                .protocols([YAS_SUBPROTOCOL])
                .max_message_size(YAS_MAX_FRAME_SIZE)
                .on_upgrade(move |socket| browser_handle_edge_ws(socket, state)),
            Err(error) => error.into_response(),
        };
    }
    if is_ws {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "unknown WebSocket endpoint",
        )
            .into_response();
    }
    yas_webserver::html_response(WEB_INDEX_HTML_BR, etag, inm, ae)
}

async fn browser_handle_edge_ws(mut ws: WebSocket, state: Arc<BrowserState>) {
    if !yas_webserver::config::authenticate_text_ws(
        &mut ws,
        &state.auth_token,
        &state.auth_throttle,
        "local",
        None,
    )
    .await
    {
        return;
    }
    let transport =
        match transport::connect_home_ipc(&state.home_socket, state.home_server_uid).await {
            Ok(transport) => transport,
            Err(error) => {
                eprintln!("yas open: cannot connect to home server: {error}");
                let _ = ws
                    .send(Message::Text("error:home server unavailable".into()))
                    .await;
                let _ = ws.close().await;
                return;
            }
        };
    let (mut home_reader, mut home_writer) = transport.split();
    if ws.send(Message::Text("ok".into())).await.is_err() {
        return;
    }
    loop {
        match ws.recv().await {
            Some(Ok(Message::Binary(preface))) if preface.as_ref() == YAS_PREFACE => {
                if home_writer.write_all(YAS_PREFACE).await.is_err() {
                    let _ = ws.close().await;
                    return;
                }
                break;
            }
            Some(Ok(Message::Ping(payload))) => {
                if ws.send(Message::Pong(payload)).await.is_err() {
                    return;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Binary(_) | Message::Text(_) | Message::Close(_)))
            | Some(Err(_))
            | None => {
                let _ = ws.close().await;
                return;
            }
        }
    }

    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut ws_to_home = tokio::spawn(async move {
        while let Some(message) = ws_rx.next().await {
            match message {
                Ok(Message::Binary(payload)) => {
                    if payload.is_empty()
                        || payload.len() > YAS_MAX_FRAME_SIZE
                        || !write_frame(&mut home_writer, &payload).await
                    {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) | Ok(Message::Text(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
            }
        }
    });
    let shutdown = state.shutdown.clone();
    let mut home_to_ws = tokio::spawn(async move {
        loop {
            tokio::select! {
                frame = read_frame(&mut home_reader) => {
                    match frame {
                        Some(payload) if payload.len() <= YAS_MAX_FRAME_SIZE => {
                            if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                _ = shutdown.notified() => break,
            }
        }
        let _ = ws_tx.close().await;
    });
    tokio::select! {
        _ = &mut ws_to_home => {}
        _ = &mut home_to_ws => {}
    }
    ws_to_home.abort();
    home_to_ws.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_edge_frame_boundary_matches_server() {
        assert_eq!(
            YAS_MAX_FRAME_SIZE,
            yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as usize
        );
    }
}
