use serde::Deserialize;
use std::net::{SocketAddr, ToSocketAddrs};

#[derive(Debug, Clone, Deserialize)]
pub struct IceConfig {
    #[serde(rename = "iceServers")]
    pub ice_servers: Vec<IceServer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IceServer {
    pub urls: UrlsField,
    pub username: Option<String>,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum UrlsField {
    Single(String),
    Multiple(Vec<String>),
}

impl UrlsField {
    pub fn iter(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            UrlsField::Single(s) => Box::new(std::iter::once(s.as_str())),
            UrlsField::Multiple(v) => Box::new(v.iter().map(|s| s.as_str())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedUrl {
    pub addr: SocketAddr,
    pub hostname: String,
    pub is_turn: bool,
    pub is_tls: bool,
    pub transport: Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
}

pub async fn fetch_ice_config(
    signal_url_base: &str,
) -> Result<IceConfig, Box<dyn std::error::Error + Send + Sync>> {
    const MAX_ICE_CONFIG_BYTES: usize = 64 * 1024;
    const MAX_ICE_SERVERS: usize = 16;
    const MAX_URLS_PER_SERVER: usize = 16;
    const MAX_ICE_FIELD_BYTES: usize = 4096;
    let base = signal_url_base
        .trim_end_matches('/')
        .replace("wss://", "https://")
        .replace("ws://", "http://");
    let url = format!("{base}/ice");
    let mut resp = reqwest::get(&url).await?.error_for_status()?;
    if resp
        .content_length()
        .is_some_and(|length| length > MAX_ICE_CONFIG_BYTES as u64)
    {
        return Err("ICE configuration exceeds its byte limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if bytes.len() + chunk.len() > MAX_ICE_CONFIG_BYTES {
            return Err("ICE configuration exceeds its byte limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let config: IceConfig = serde_json::from_slice(&bytes)?;
    if config.ice_servers.len() > MAX_ICE_SERVERS {
        return Err("ICE configuration has too many servers".into());
    }
    for server in &config.ice_servers {
        let urls: Vec<_> = server.urls.iter().collect();
        if urls.len() > MAX_URLS_PER_SERVER
            || urls.iter().any(|value| value.len() > MAX_ICE_FIELD_BYTES)
            || server
                .username
                .as_ref()
                .is_some_and(|value| value.len() > MAX_ICE_FIELD_BYTES)
            || server
                .credential
                .as_ref()
                .is_some_and(|value| value.len() > MAX_ICE_FIELD_BYTES)
        {
            return Err("ICE configuration field exceeds its limit".into());
        }
    }
    Ok(config)
}

pub fn parse_ice_url(url: &str) -> Option<ParsedUrl> {
    let (scheme, rest) = url.split_once(':')?;
    let is_turn = scheme == "turn" || scheme == "turns";
    let is_tls = scheme == "turns";

    let (host_port, query) = if let Some((hp, q)) = rest.split_once('?') {
        (hp, Some(q))
    } else {
        (rest, None)
    };

    let transport = if let Some(q) = query {
        if q.contains("transport=tcp") {
            Transport::Tcp
        } else {
            Transport::Udp
        }
    } else if is_tls {
        Transport::Tcp
    } else {
        Transport::Udp
    };

    let default_port = if is_tls { 5349 } else { 3478 };
    let (hostname, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        (h.to_owned(), p.parse().unwrap_or(default_port))
    } else {
        (host_port.to_owned(), default_port)
    };

    let addr_str = format!("{hostname}:{port}");
    let addr = addr_str.to_socket_addrs().ok()?.next()?;

    Some(ParsedUrl {
        addr,
        hostname,
        is_turn,
        is_tls,
        transport,
    })
}

pub fn collect_servers(config: &IceConfig) -> (Vec<SocketAddr>, Vec<TurnServerInfo>) {
    let mut stun_servers = Vec::with_capacity(config.ice_servers.len());
    let mut turn_servers = Vec::with_capacity(config.ice_servers.len());

    for server in &config.ice_servers {
        for url in server.urls.iter() {
            if let Some(parsed) = parse_ice_url(url) {
                if parsed.is_turn {
                    if let (Some(username), Some(credential)) =
                        (&server.username, &server.credential)
                    {
                        turn_servers.push(TurnServerInfo {
                            addr: parsed.addr,
                            hostname: parsed.hostname,
                            username: username.clone(),
                            credential: credential.clone(),
                            transport: parsed.transport,
                            tls: parsed.is_tls,
                        });
                    }
                } else {
                    stun_servers.push(parsed.addr);
                }
            }
        }
    }

    (stun_servers, turn_servers)
}

#[derive(Debug, Clone)]
pub struct TurnServerInfo {
    pub addr: SocketAddr,
    pub hostname: String,
    pub username: String,
    pub credential: String,
    pub transport: Transport,
    pub tls: bool,
}
