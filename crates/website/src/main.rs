use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::{Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

const INSTALL_SH: &str = include_str!("../../../install.sh");
const INSTALL_PS1: &str = include_str!("../../../install.ps1");
const SKILL: &str = include_str!("../../../SKILL.md");
const MAX_PAYLOAD_BYTES: usize = 65_536;
const REDIS_TIMEOUT: Duration = Duration::from_secs(2);
const SESSION_TTL_SECONDS: u64 = 600;
const MESSAGE_TEMPLATE: &str =
    "Terminals at https://yas.run/s#psk={secret}\nRead-only: https://yas.run/s#psk={ro_secret}";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Producer,
    Consumer,
}

impl Role {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "producer" => Some(Self::Producer),
            "consumer" => Some(Self::Consumer),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Producer => "producer",
            Self::Consumer => "consumer",
        }
    }
}

#[derive(Clone)]
struct Peer {
    role: Role,
    tx: mpsc::UnboundedSender<Message>,
}

type Channels = Arc<Mutex<HashMap<String, HashMap<String, Peer>>>>;

#[derive(Clone)]
struct AppState {
    channels: Channels,
    hub: HubStore,
    message_template: Arc<str>,
    ice: IceState,
}

#[derive(Clone)]
struct HubStore {
    client: redis::Client,
    commands: redis::aio::ConnectionManager,
}

impl HubStore {
    async fn open(url: &str) -> Result<Self, String> {
        let client = redis::Client::open(url).map_err(|error| format!("redis: {error}"))?;
        let commands = client
            .get_connection_manager()
            .await
            .map_err(|error| format!("redis: {error}"))?;
        Ok(Self { client, commands })
    }

    async fn ping(&self) -> Result<(), String> {
        let mut connection = self.commands.clone();
        tokio::time::timeout(REDIS_TIMEOUT, async move {
            let _: String = redis::cmd("PING")
                .query_async(&mut connection)
                .await
                .map_err(|error| format!("redis PING: {error}"))?;
            Ok(())
        })
        .await
        .map_err(|_| "redis PING timed out".to_string())?
    }

    async fn register(
        &self,
        channel_id: &str,
        role: Role,
        session_id: &str,
    ) -> Result<Vec<String>, String> {
        let role_key = member_key(channel_id, role);
        let session_alive_key = alive_key(channel_id, session_id);
        let other_key = member_key(
            channel_id,
            match role {
                Role::Producer => Role::Consumer,
                Role::Consumer => Role::Producer,
            },
        );
        let mut connection = self.commands.clone();
        tokio::time::timeout(REDIS_TIMEOUT, async move {
            let _: usize = connection
                .sadd(&role_key, session_id)
                .await
                .map_err(|error| format!("redis SADD: {error}"))?;
            let _: bool = connection
                .expire(&role_key, SESSION_TTL_SECONDS as i64)
                .await
                .map_err(|error| format!("redis EXPIRE: {error}"))?;
            let _: () = connection
                .set_ex(&session_alive_key, "1", SESSION_TTL_SECONDS)
                .await
                .map_err(|error| format!("redis SETEX: {error}"))?;

            let members: Vec<String> = connection
                .smembers(&other_key)
                .await
                .map_err(|error| format!("redis SMEMBERS: {error}"))?;
            if members.is_empty() {
                return Ok(Vec::new());
            }
            let keys = members
                .iter()
                .map(|session| alive_key(channel_id, session))
                .collect::<Vec<_>>();
            let liveness: Vec<Option<String>> = redis::cmd("MGET")
                .arg(&keys)
                .query_async(&mut connection)
                .await
                .map_err(|error| format!("redis MGET: {error}"))?;
            let mut live = Vec::new();
            let mut stale = Vec::new();
            for (session, alive) in members.into_iter().zip(liveness) {
                if alive.is_some() {
                    live.push(session);
                } else {
                    stale.push(session);
                }
            }
            if !stale.is_empty() {
                let _: usize = connection
                    .srem(&other_key, stale)
                    .await
                    .map_err(|error| format!("redis SREM: {error}"))?;
            }
            Ok(live)
        })
        .await
        .map_err(|_| "redis registration timed out".to_string())?
    }

    async fn refresh(&self, channel_id: &str, role: Role, session_id: &str) -> Result<(), String> {
        let member_key = member_key(channel_id, role);
        let alive_key = alive_key(channel_id, session_id);
        let mut connection = self.commands.clone();
        tokio::time::timeout(REDIS_TIMEOUT, async move {
            let _: bool = connection
                .expire(member_key, SESSION_TTL_SECONDS as i64)
                .await
                .map_err(|error| format!("redis EXPIRE: {error}"))?;
            let _: bool = connection
                .expire(alive_key, SESSION_TTL_SECONDS as i64)
                .await
                .map_err(|error| format!("redis EXPIRE: {error}"))?;
            Ok(())
        })
        .await
        .map_err(|_| "redis refresh timed out".to_string())?
    }

    async fn unregister(&self, channel_id: &str, role: Role, session_id: &str) {
        let member_key = member_key(channel_id, role);
        let alive_key = alive_key(channel_id, session_id);
        let mut connection = self.commands.clone();
        let _ = tokio::time::timeout(REDIS_TIMEOUT, async move {
            let _: redis::RedisResult<usize> = connection.srem(member_key, session_id).await;
            let _: redis::RedisResult<usize> = connection.del(alive_key).await;
        })
        .await;
    }

    async fn publish_presence(
        &self,
        channel_id: &str,
        message_type: &'static str,
        role: Role,
        session_id: &str,
    ) -> Result<(), String> {
        let topic = presence_topic(channel_id);
        let payload = json!({
            "type": message_type,
            "role": role.as_str(),
            "sessionId": session_id,
        })
        .to_string();
        self.publish(topic, payload).await
    }

    async fn relay(&self, channel_id: &str, target: &str, payload: String) -> Result<(), String> {
        let topic = session_topic(channel_id, target);
        let envelope = json!({
            "channelId": channel_id,
            "targetSessionId": target,
            "payload": payload,
        })
        .to_string();
        self.publish(topic, envelope).await
    }

    async fn publish(&self, topic: String, payload: String) -> Result<(), String> {
        let mut connection = self.commands.clone();
        tokio::time::timeout(REDIS_TIMEOUT, async move {
            let _: usize = connection
                .publish(topic, payload)
                .await
                .map_err(|error| format!("redis PUBLISH: {error}"))?;
            Ok(())
        })
        .await
        .map_err(|_| "redis publish timed out".to_string())?
    }

    async fn start_listener(&self, channels: Channels) -> Result<(), String> {
        let pubsub = subscribe(&self.client).await?;
        let client = self.client.clone();
        tokio::spawn(async move {
            let mut next = Some(pubsub);
            loop {
                let mut pubsub = match next.take() {
                    Some(pubsub) => pubsub,
                    None => match subscribe(&client).await {
                        Ok(pubsub) => pubsub,
                        Err(error) => {
                            eprintln!("yas-website: {error}");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    },
                };
                {
                    let mut messages = pubsub.on_message();
                    while let Some(message) = messages.next().await {
                        let topic = message.get_channel_name().to_string();
                        let Ok(payload) = message.get_payload::<String>() else {
                            continue;
                        };
                        dispatch_redis(&channels, &topic, &payload).await;
                    }
                }
                eprintln!("yas-website: redis subscription ended; reconnecting");
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
        Ok(())
    }
}

fn redis_key(kind: &str, channel_id: &str, suffix: Option<&str>) -> String {
    match suffix {
        Some(suffix) => format!("yas:{kind}:{channel_id}:{suffix}"),
        None => format!("yas:{kind}:{channel_id}"),
    }
}

fn member_key(channel_id: &str, role: Role) -> String {
    redis_key(role.as_str(), channel_id, None)
}

fn alive_key(channel_id: &str, session_id: &str) -> String {
    redis_key("alive", channel_id, Some(session_id))
}

fn presence_topic(channel_id: &str) -> String {
    redis_key("presence", channel_id, None)
}

fn session_topic(channel_id: &str, session_id: &str) -> String {
    redis_key("to_session", channel_id, Some(session_id))
}

async fn subscribe(client: &redis::Client) -> Result<redis::aio::PubSub, String> {
    let mut pubsub = client
        .get_async_pubsub()
        .await
        .map_err(|error| format!("redis pubsub: {error}"))?;
    pubsub
        .psubscribe("yas:presence:*")
        .await
        .map_err(|error| format!("redis PSUBSCRIBE: {error}"))?;
    pubsub
        .psubscribe("yas:to_session:*")
        .await
        .map_err(|error| format!("redis PSUBSCRIBE: {error}"))?;
    Ok(pubsub)
}

async fn dispatch_redis(channels: &Channels, topic: &str, payload: &str) {
    if let Some(channel_id) = topic.strip_prefix("yas:presence:") {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        let source = value.get("sessionId").and_then(Value::as_str);
        let listeners = channels
            .lock()
            .await
            .get(channel_id)
            .map(|channel| {
                channel
                    .iter()
                    .filter(|(session, _)| Some(session.as_str()) != source)
                    .map(|(_, peer)| peer.tx.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for listener in listeners {
            send_json(&listener, value.clone());
        }
        return;
    }

    if !topic.starts_with("yas:to_session:") {
        return;
    }
    let Ok(envelope) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    let Some(channel_id) = envelope.get("channelId").and_then(Value::as_str) else {
        return;
    };
    let Some(target) = envelope.get("targetSessionId").and_then(Value::as_str) else {
        return;
    };
    let Some(payload) = envelope.get("payload").and_then(Value::as_str) else {
        return;
    };
    let target = channels
        .lock()
        .await
        .get(channel_id)
        .and_then(|channel| channel.get(target))
        .map(|peer| peer.tx.clone());
    if let Some(target) = target {
        let _ = target.send(Message::Text(payload.to_string().into()));
    }
}

#[derive(Clone)]
struct IceState {
    client: reqwest::Client,
    token_id: Option<String>,
    api_token: Option<String>,
    cached: Arc<Mutex<Option<(Instant, Value)>>>,
}

impl IceState {
    fn from_env() -> Self {
        Self {
            client: reqwest::Client::new(),
            token_id: std::env::var("CF_TURN_TOKEN_ID").ok(),
            api_token: std::env::var("CF_TURN_API_TOKEN").ok(),
            cached: Arc::new(Mutex::new(None)),
        }
    }

    async fn servers(&self) -> Value {
        let fallback = || {
            json!({
                "iceServers": [
                    { "urls": "stun:stun.l.google.com:19302" },
                    { "urls": "stun:stun1.l.google.com:19302" }
                ]
            })
        };
        let (Some(token_id), Some(api_token)) = (&self.token_id, &self.api_token) else {
            return fallback();
        };

        if let Some((expires, value)) = self.cached.lock().await.as_ref()
            && *expires > Instant::now()
        {
            return value.clone();
        }

        let url = format!(
            "https://rtc.live.cloudflare.com/v1/turn/keys/{token_id}/credentials/generate-ice-servers"
        );
        let response = self
            .client
            .post(url)
            .bearer_auth(api_token)
            .json(&json!({ "ttl": 86_400 }))
            .send()
            .await;
        let value = match response {
            Ok(response) if response.status().is_success() => response.json::<Value>().await.ok(),
            _ => None,
        };
        let Some(value) = value else {
            return fallback();
        };
        *self.cached.lock().await =
            Some((Instant::now() + Duration::from_secs(43_200), value.clone()));
        value
    }
}

#[derive(Clone, Copy)]
enum Installer {
    Shell,
    PowerShell,
}

fn accepts_html(accept: Option<&str>) -> bool {
    accept.is_some_and(|value| {
        value.split(',').any(|range| {
            range
                .split(';')
                .next()
                .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/html"))
        })
    })
}

fn installer_for_request(headers: &HeaderMap) -> Option<Installer> {
    if accepts_html(
        headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok()),
    ) {
        return None;
    }
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ua.contains("powershell") {
        return Some(Installer::PowerShell);
    }
    Some(Installer::Shell)
}

fn script_response(installer: Installer) -> Response {
    let (body, content_type) = match installer {
        Installer::Shell => (INSTALL_SH, "text/x-shellscript; charset=utf-8"),
        Installer::PowerShell => (INSTALL_PS1, "text/plain; charset=utf-8"),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn root(headers: HeaderMap) -> Response {
    match installer_for_request(&headers) {
        Some(installer) => script_response(installer),
        None => asset_response("index.html"),
    }
}

async fn install_sh() -> Response {
    script_response(Installer::Shell)
}

async fn install_ps1() -> Response {
    script_response(Installer::PowerShell)
}

async fn skill() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SKILL,
    )
        .into_response()
}

async fn release_asset(Path(file): Path<String>) -> Response {
    if file.is_empty()
        || !file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    Redirect::temporary(&format!(
        "https://github.com/pcarrier/yas/releases/latest/download/{file}"
    ))
    .into_response()
}

async fn health(State(state): State<AppState>) -> Response {
    match state.hub.ping().await {
        Ok(()) => "ok".into_response(),
        Err(error) => (StatusCode::SERVICE_UNAVAILABLE, error).into_response(),
    }
}

async fn message(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "template": &*state.message_template }))
}

async fn ice(State(state): State<AppState>) -> Json<Value> {
    Json(state.ice.servers().await)
}

async fn channel(
    State(state): State<AppState>,
    Path((channel_id, role)): Path<(String, String)>,
    ws: WebSocketUpgrade,
) -> Response {
    if channel_id.len() != 64 || !channel_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let Some(role) = Role::parse(&role) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    ws.max_message_size(MAX_PAYLOAD_BYTES)
        .on_upgrade(move |socket| {
            peer(
                socket,
                state.channels,
                state.hub,
                channel_id.to_ascii_lowercase(),
                role,
            )
        })
}

async fn peer(
    socket: WebSocket,
    channels: Channels,
    hub: HubStore,
    channel_id: String,
    role: Role,
) {
    let session_id = Uuid::new_v4().to_string();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_tx.send(message).await.is_err() {
                break;
            }
        }
    });

    let local_peers = {
        let mut all = channels.lock().await;
        let channel = all.entry(channel_id.clone()).or_default();
        let local_peers = channel
            .iter()
            .filter(|(_, peer)| peer.role != role)
            .map(|(id, peer)| (id.clone(), peer.role))
            .collect::<Vec<_>>();
        channel.insert(
            session_id.clone(),
            Peer {
                role,
                tx: tx.clone(),
            },
        );
        local_peers
    };

    let remote_peers = match hub.register(&channel_id, role, &session_id).await {
        Ok(peers) => peers,
        Err(error) => {
            let mut all = channels.lock().await;
            if let Some(channel) = all.get_mut(&channel_id) {
                channel.remove(&session_id);
                if channel.is_empty() {
                    all.remove(&channel_id);
                }
            }
            drop(all);
            send_error(&tx, "hub cannot reach its presence store; retry");
            let _ = tx.send(Message::Close(None));
            eprintln!("yas-website: register {session_id}: {error}");
            drop(tx);
            let _ = writer.await;
            return;
        }
    };

    send_json(
        &tx,
        json!({
            "type": "registered",
            "channelId": channel_id,
            "role": role.as_str(),
            "sessionId": session_id,
        }),
    );

    let mut existing_peers = local_peers;
    existing_peers.extend(remote_peers.into_iter().map(|id| {
        (
            id,
            match role {
                Role::Producer => Role::Consumer,
                Role::Consumer => Role::Producer,
            },
        )
    }));
    existing_peers.sort_by(|a, b| a.0.cmp(&b.0));
    existing_peers.dedup_by(|a, b| a.0 == b.0);
    for (id, peer_role) in existing_peers {
        send_json(
            &tx,
            json!({ "type": "peer_joined", "role": peer_role.as_str(), "sessionId": id }),
        );
    }
    if let Err(error) = hub
        .publish_presence(&channel_id, "peer_joined", role, &session_id)
        .await
    {
        eprintln!("yas-website: presence join {session_id}: {error}");
    }

    let refresh_hub = hub.clone();
    let refresh_channel = channel_id.clone();
    let refresh_session = session_id.clone();
    let refresh = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(SESSION_TTL_SECONDS / 2));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = refresh_hub
                .refresh(&refresh_channel, role, &refresh_session)
                .await
            {
                eprintln!("yas-website: refresh {refresh_session}: {error}");
            }
        }
    });

    while let Some(Ok(message)) = ws_rx.next().await {
        let bytes = match message {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        if bytes.len() > MAX_PAYLOAD_BYTES {
            send_error(&tx, "payload too large");
            continue;
        }
        let outer: SignedMessage = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                send_error(&tx, "invalid json");
                continue;
            }
        };
        let Some(target) = outer.target else {
            send_error(&tx, "missing target");
            continue;
        };
        let payload = match open_signed(&outer.signed, &channel_id) {
            Ok(payload) => payload,
            Err(message) => {
                send_error(&tx, message);
                continue;
            }
        };
        let payload = json!({ "type": "signal", "from": session_id, "data": payload }).to_string();
        if let Err(error) = hub.relay(&channel_id, &target, payload).await {
            eprintln!("yas-website: relay to {target}: {error}");
            send_error(&tx, "hub could not relay the signal; retry");
        }
    }

    refresh.abort();
    {
        let mut all = channels.lock().await;
        let Some(channel) = all.get_mut(&channel_id) else {
            writer.abort();
            return;
        };
        channel.remove(&session_id);
        if channel.is_empty() {
            all.remove(&channel_id);
        }
    }
    hub.unregister(&channel_id, role, &session_id).await;
    if let Err(error) = hub
        .publish_presence(&channel_id, "peer_left", role, &session_id)
        .await
    {
        eprintln!("yas-website: presence leave {session_id}: {error}");
    }
    writer.abort();
}

#[derive(Deserialize)]
struct SignedMessage {
    signed: String,
    target: Option<String>,
}

fn open_signed(signed: &str, channel_id: &str) -> Result<Value, &'static str> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(signed)
        .map_err(|_| "signature verification failed")?;
    if bytes.len() < 64 {
        return Err("signature verification failed");
    }
    let signature =
        Signature::from_slice(&bytes[..64]).map_err(|_| "signature verification failed")?;
    let public = decode_hex_32(channel_id).ok_or("signature verification failed")?;
    let key = VerifyingKey::from_bytes(&public).map_err(|_| "signature verification failed")?;
    key.verify_strict(&bytes[64..], &signature)
        .map_err(|_| "signature verification failed")?;
    serde_json::from_slice(&bytes[64..]).map_err(|_| "signed payload is not valid json")
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn send_json(tx: &mpsc::UnboundedSender<Message>, value: Value) {
    let _ = tx.send(Message::Text(value.to_string().into()));
}

fn send_error(tx: &mpsc::UnboundedSender<Message>, message: &'static str) {
    send_json(tx, json!({ "type": "error", "message": message }));
}

async fn static_asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = match path {
        "s" | "s/" => "s/index.html",
        path => path,
    };
    asset_response(path)
}

fn asset_response(path: &str) -> Response {
    let Some((_, bytes)) = embedded::ASSETS.iter().find(|(name, _)| *name == path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let content_type = match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    let cache = if path == "index.html" || path.ends_with("/index.html") {
        "no-cache"
    } else if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    let mut response = Response::new(Body::from(*bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

async fn cors(request: Request, next: Next) -> Response {
    let is_options = request.method() == Method::OPTIONS;
    let mut response = if is_options {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, OPTIONS"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
    response
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/install.sh", get(install_sh))
        .route("/install.ps1", get(install_ps1))
        .route("/SKILL.md", get(skill))
        .route("/ext/{file}", get(release_asset))
        .route("/health", get(health))
        .route("/message", get(message))
        .route("/ice", get(ice))
        .route("/channel/{channel_id}/{role}", get(channel))
        .fallback(static_asset)
        .layer(middleware::from_fn(cors))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("YAS_WEBSITE_ADDR").unwrap_or_else(|_| "0.0.0.0:8000".into());
    let address: SocketAddr = address.parse()?;
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let channels: Channels = Default::default();
    let hub = HubStore::open(&redis_url)
        .await
        .map_err(std::io::Error::other)?;
    hub.start_listener(channels.clone())
        .await
        .map_err(std::io::Error::other)?;
    let state = AppState {
        channels,
        hub,
        message_template: std::env::var("MESSAGE_TEMPLATE")
            .unwrap_or_else(|_| MESSAGE_TEMPLATE.into())
            .into(),
        ice: IceState::from_env(),
    };
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!("yas-website listening on {address}");
    axum::serve(listener, app(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn serves_html_only_when_accepted() {
        let mut headers = HeaderMap::new();
        assert!(matches!(
            installer_for_request(&headers),
            Some(Installer::Shell)
        ));

        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_static("Mozilla/5.0 WindowsPowerShell/5.1"),
        );
        assert!(matches!(
            installer_for_request(&headers),
            Some(Installer::PowerShell)
        ));

        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html,application/xhtml+xml;q=0.9"),
        );
        assert!(installer_for_request(&headers).is_none());
    }

    #[test]
    fn verifies_signed_json() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let payload = br#"{"offer":"test"}"#;
        let signature = key.sign(payload);
        let mut signed = signature.to_bytes().to_vec();
        signed.extend_from_slice(payload);
        let encoded = base64::engine::general_purpose::STANDARD.encode(signed);
        let channel = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            open_signed(&encoded, &channel).unwrap(),
            json!({ "offer": "test" })
        );
    }

    #[tokio::test]
    async fn relays_between_instances_through_redis() {
        let Ok(redis_url) = std::env::var("YAS_TEST_REDIS_URL") else {
            return;
        };
        let first = HubStore::open(&redis_url).await.unwrap();
        let second = HubStore::open(&redis_url).await.unwrap();
        let channels: Channels = Default::default();
        second.start_listener(channels.clone()).await.unwrap();

        let channel_id = format!("{:064x}", Uuid::new_v4().as_u128());
        let producer = Uuid::new_v4().to_string();
        let consumer = Uuid::new_v4().to_string();
        assert!(
            first
                .register(&channel_id, Role::Producer, &producer)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            second
                .register(&channel_id, Role::Consumer, &consumer)
                .await
                .unwrap(),
            vec![producer.clone()]
        );

        let (tx, mut rx) = mpsc::unbounded_channel();
        channels.lock().await.insert(
            channel_id.clone(),
            HashMap::from([(
                consumer.clone(),
                Peer {
                    role: Role::Consumer,
                    tx,
                },
            )]),
        );
        first
            .relay(&channel_id, &consumer, "cross-instance".into())
            .await
            .unwrap();
        let message = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.into_text().unwrap(), "cross-instance");

        first
            .unregister(&channel_id, Role::Producer, &producer)
            .await;
        second
            .unregister(&channel_id, Role::Consumer, &consumer)
            .await;
    }
}
