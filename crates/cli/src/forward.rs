//! `yas forward` — port forwarding over the yas connection (docs/design/net.md § Client: `yas forward`).
//! `ssh -L` over any yas transport, plus the UDP case ssh has never had.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::yas_net::{self, Connection, DEFAULT_BIND, DatagramFlow, OnOpen, TlsConfig, bracket};

// --------------------------------------------------------------------------- Specs ---------------------------------------------------------------------------

/// Which kind of socket a spec forwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tcp,
    Udp,
    /// Local plaintext in, TLS to the target, terminated on the server.
    Tls,
}

/// One forward: a local listener and the target it relays to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: Kind,
    pub bind: String,
    pub local_port: u16,
    pub host: String,
    pub host_port: u16,
}

impl std::fmt::Display for Spec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Kind::Udp => write!(f, "udp/")?,
            Kind::Tls => write!(f, "tls/")?,
            Kind::Tcp => {}
        }
        if self.bind != DEFAULT_BIND {
            write!(f, "{}:", bracket(&self.bind))?;
        }
        write!(
            f,
            "{}:{}:{}",
            self.local_port,
            bracket(&self.host),
            self.host_port
        )
    }
}

/// Parse `[kind/][bind_address:]local_port:host:host_port`.
pub fn parse_spec(s: &str) -> Result<Spec, String> {
    let bad = |what: &str| Err(format!("{s}: {what}"));
    let (kind, rest) = match s.split_once('/') {
        Some(("tcp", rest)) => (Kind::Tcp, rest),
        Some(("udp", rest)) => (Kind::Udp, rest),
        Some(("tls", rest)) => (Kind::Tls, rest),
        Some((other, _)) => {
            return bad(&format!("unknown kind `{other}` (want tcp, udp or tls)"));
        }
        None => (Kind::Tcp, s),
    };
    // Colon-separated fields, with `[...]` atomic so a bracketed IPv6 address is one field and not several.
    let Some(fields) = split_fields(rest) else {
        return bad("unterminated [address]");
    };
    let (bind, port_str, host, host_port_str) = match fields.as_slice() {
        [port, host, host_port] => (DEFAULT_BIND.to_string(), port, host, host_port),
        [bind, port, host, host_port] => (bind.clone(), port, host, host_port),
        _ => return bad("want [kind/][bind:]port:host:hostport"),
    };
    let host_port: u16 = match host_port_str.parse() {
        Ok(p) if p > 0 => p,
        _ => return bad(&format!("bad target port `{host_port_str}`")),
    };
    if host.is_empty() {
        return bad("empty target host");
    }
    // Port 0 means "pick one and tell me", which is what makes a forward scriptable without hunting for a free port first.
    let local_port: u16 = match port_str.parse() {
        Ok(p) => p,
        Err(_) => return bad(&format!("bad local port `{port_str}`")),
    };
    if bind.is_empty() {
        return bad("empty bind address");
    }
    Ok(Spec {
        kind,
        bind,
        local_port,
        host: host.clone(),
        host_port,
    })
}

/// Split on `:`, treating a leading `[...]` in each field as atomic so IPv6 literals survive.
fn split_fields(s: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_bracket = false;
    let mut closed = false;
    for c in s.chars() {
        match c {
            '[' if !in_bracket && cur.is_empty() && !closed => in_bracket = true,
            ']' if in_bracket => {
                in_bracket = false;
                closed = true;
            }
            ':' if !in_bracket => {
                fields.push(std::mem::take(&mut cur));
                closed = false;
            }
            // Trailing junk after `]` (as in `[::1]x:80`) is malformed, not silently concatenated.
            _ if closed => return None,
            _ => cur.push(c),
        }
    }
    if in_bracket {
        return None;
    }
    fields.push(cur);
    Some(fields)
}

/// TLS options for `tls/` specs.
#[derive(Clone, Debug, Default)]
pub struct TlsOpts {
    /// ALPN protocols to offer, in preference order.
    pub alpn: Vec<String>,
    /// Skip certificate verification.
    pub insecure: bool,
}

impl TlsOpts {
    fn native(&self) -> TlsConfig {
        TlsConfig {
            alpn: self.alpn.clone(),
            insecure: self.insecure,
        }
    }
}

// --------------------------------------------------------------------------- Entry point ---------------------------------------------------------------------------

/// Bind every listener, then serve.
pub async fn cmd_forward(
    on: Option<&str>,
    hub: &str,
    specs: Vec<Spec>,
    tls: TlsOpts,
) -> Result<i32, String> {
    if specs.is_empty() {
        return Err(
            "nothing to forward: pass a spec, or --all with entries in yas.forwards".into(),
        );
    }

    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    for spec in &specs {
        let addr = format!("{}:{}", spec.bind, spec.local_port);
        match spec.kind {
            // A `tls/` forward listens in plaintext exactly like `tcp/`; the difference is one flag on the open.
            Kind::Tcp | Kind::Tls => {
                let listener = tokio::net::TcpListener::bind(&addr)
                    .await
                    .map_err(|e| format!("cannot bind {addr}: {e}"))?;
                tcp.push((spec.clone(), listener));
            }
            Kind::Udp => {
                let socket = tokio::net::UdpSocket::bind(&addr)
                    .await
                    .map_err(|e| format!("cannot bind {addr}/udp: {e}"))?;
                udp.push((spec.clone(), socket));
            }
        }
    }

    let conn = Connection::connect(on, hub).await?;

    for (spec, listener) in tcp {
        let local = listener
            .local_addr()
            .map_err(|e| format!("cannot read local address: {e}"))?;
        report(&spec, local);
        let conn = conn.clone();
        let tls = tls.clone();
        tokio::spawn(async move { serve_tcp(listener, spec, conn, tls).await });
    }
    for (spec, socket) in udp {
        let local = socket
            .local_addr()
            .map_err(|e| format!("cannot read local address: {e}"))?;
        report(&spec, local);
        let conn = conn.clone();
        tokio::spawn(async move { serve_udp(socket, spec, conn).await });
    }

    // The reader owns the rest of the process's life: when the connection drops, every forward goes with it.
    conn.wait_closed().await;
    Ok(0)
}

fn report(spec: &Spec, local: SocketAddr) {
    let kind = match spec.kind {
        Kind::Tcp => "tcp",
        Kind::Udp => "udp",
        Kind::Tls => "tcp → tls",
    };
    eprintln!(
        "yas: forwarding {kind} {local} → {}:{}",
        spec.host, spec.host_port
    );
}

// --------------------------------------------------------------------------- TCP ---------------------------------------------------------------------------

async fn serve_tcp(listener: tokio::net::TcpListener, spec: Spec, conn: Connection, tls: TlsOpts) {
    // The negotiated protocol is worth saying once — per connection it would be noise on a busy forward.
    let announced = Arc::new(std::sync::atomic::AtomicBool::new(false));
    loop {
        let (local, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("yas: accept failed on {}: {e}", spec);
                return;
            }
        };
        let conn = conn.clone();
        let spec = spec.clone();
        let tls = tls.clone();
        let announced = announced.clone();
        tokio::spawn(async move {
            if let Err(e) = relay_tcp(local, spec, conn, tls, announced).await {
                eprintln!("yas: {peer}: {e}");
            }
        });
    }
}

async fn relay_tcp(
    local: tokio::net::TcpStream,
    spec: Spec,
    conn: Connection,
    tls: TlsOpts,
    announced: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    yas_net::relay_tcp(
        local,
        conn,
        spec.host,
        spec.host_port,
        (spec.kind == Kind::Tls).then(|| tls.native()),
        OnOpen::Report {
            announce_alpn: (spec.kind == Kind::Tls).then_some(announced),
        },
    )
    .await
}

// --------------------------------------------------------------------------- UDP ---------------------------------------------------------------------------

/// One flow per distinct local source address, created on that source's first datagram and torn down by the server's idle timeout — the NAT model, because it is the only one that demultiplexes replies back to the right sender (docs/design/net.md § Client: `yas forward`).
async fn serve_udp(socket: tokio::net::UdpSocket, spec: Spec, conn: Connection) {
    let socket = Arc::new(socket);
    let mut flows: HashMap<SocketAddr, DatagramFlow> = HashMap::new();
    // The protocol cap is the IPv4 UDP payload cap. Keep one full u16-sized
    // receive buffer so an oversized local datagram is rejected, never silently
    // truncated into a different packet.
    let mut buf = vec![0u8; usize::from(u16::MAX) + 1];
    loop {
        let (n, from) = match socket.recv_from(&mut buf).await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("yas: recv failed on {}: {e}", spec);
                return;
            }
        };
        flows.retain(|_, flow| !flow.is_closed());
        if n > conn.max_datagram_payload() {
            eprintln!(
                "yas: dropping {n}-byte UDP datagram from {from}; negotiated maximum is {}",
                conn.max_datagram_payload()
            );
            continue;
        }
        // A closed flow leaves a dead sender behind; replace it rather than dropping the datagram, so a source that goes quiet past the idle timeout and comes back simply gets a new flow.
        let live = flows.get(&from).is_some_and(|flow| !flow.is_closed());
        if !live {
            match start_udp_flow(socket.clone(), from, &spec, conn.clone()).await {
                Ok(flow) => {
                    flows.insert(from, flow);
                }
                Err(error) => {
                    eprintln!("yas: cannot open UDP flow for {from}: {error}");
                    continue;
                }
            }
        }
        if let Some(flow) = flows.get(&from)
            && let Err(error) = flow.send(&buf[..n]).await
        {
            eprintln!("yas: UDP flow for {from}: {error}");
            if let Some(flow) = flows.remove(&from) {
                flow.close_in_background();
            }
        }
    }
}

/// Open one flow and spawn its pump.
async fn start_udp_flow(
    socket: Arc<tokio::net::UdpSocket>,
    from: SocketAddr,
    spec: &Spec,
    conn: Connection,
) -> Result<DatagramFlow, String> {
    let flow = conn
        .open_udp(&spec.host, spec.host_port)
        .await
        .map_err(|error| error.to_string())?;
    let pump = flow.clone();
    let guard = conn.relay_guard();
    let target = format!("{}:{}", spec.host, spec.host_port);
    tokio::spawn(async move {
        let _guard = guard;
        loop {
            match pump.recv().await {
                Ok(Some(payload)) => {
                    if socket.send_to(&payload, from).await.is_err() {
                        pump.close_in_background();
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    eprintln!("yas: UDP flow {target}: {error}");
                    break;
                }
            }
        }
        if let Some(stats) = pump.final_stats() {
            let drops = stats.client_oversized_drops
                + stats.peer_oversized_drops
                + stats.client_congestive_drops
                + stats.peer_congestive_drops;
            if drops != 0 || stats.transport_errors != 0 {
                eprintln!(
                    "yas: UDP flow {target} closed: drops={drops}, transport-errors={}",
                    stats.transport_errors
                );
            }
        }
        pump.retire();
    });
    Ok(flow)
}

// --------------------------------------------------------------------------- The named list ---------------------------------------------------------------------------

use yas_webserver::config::{ForwardEntry, modify_forwards, read_forwards_full};

/// Resolve what to forward: explicit specs, or every enabled entry in `yas.forwards` under `--all`.
pub fn resolve_specs(args: &[String], all: bool) -> Result<Vec<Spec>, String> {
    let mut specs = Vec::new();
    if all {
        for entry in read_forwards_full().into_iter().filter(|e| !e.disabled) {
            let spec = parse_spec(&entry.spec)
                .map_err(|e| format!("yas.forwards entry `{}`: {e}", entry.name))?;
            specs.push(spec);
        }
        if specs.is_empty() {
            return Err("no enabled entries in yas.forwards".into());
        }
    }
    for arg in args {
        specs.push(parse_spec(arg)?);
    }
    Ok(specs)
}

/// `yas forward add NAME SPEC` — add or update one entry.
pub fn cmd_add(name: &str, spec: &str) -> Result<i32, String> {
    // Shared with yas.remotes / yas.roots: same file shape, same
    // space-delimited config verbs, so the same rule.
    if !yas_webserver::config::valid_entry_name(name) {
        return Err(format!(
            "bad entry name `{name}` — no whitespace, `=`, or leading `#`"
        ));
    }
    // Validate before persisting: an entry that cannot parse is a `--all` that refuses to start, discovered much later.
    let parsed = parse_spec(spec)?;
    let stored = parsed.to_string();
    modify_forwards(|entries| {
        if let Some(existing) = entries.iter_mut().find(|e| e.name == name) {
            existing.spec = stored.clone();
            existing.disabled = false;
        } else {
            entries.push(ForwardEntry {
                name: name.to_string(),
                spec: stored.clone(),
                disabled: false,
            });
        }
    });
    println!("{name} = {stored}");
    Ok(0)
}

/// `yas forward list` — every entry, disabled ones marked.
pub fn cmd_list() -> Result<i32, String> {
    let entries = read_forwards_full();
    if entries.is_empty() {
        eprintln!("yas: no forwards configured (yas forward add NAME SPEC)");
        return Ok(0);
    }
    let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
    for e in entries {
        let mark = if e.disabled { " (disabled)" } else { "" };
        println!("{:<width$}  {}{mark}", e.name, e.spec, width = width);
    }
    Ok(0)
}

/// `yas forward rm NAME` — remove one entry.
pub fn cmd_rm(name: &str) -> Result<i32, String> {
    let before = read_forwards_full().len();
    modify_forwards(|entries| entries.retain(|e| e.name != name));
    if read_forwards_full().len() == before {
        return Err(format!("no such forward: {name}"));
    }
    Ok(0)
}

/// `yas forward toggle NAME` — disable or re-enable without removing, the `yas remote toggle` convention.
pub fn cmd_toggle(name: &str) -> Result<i32, String> {
    let mut found = false;
    modify_forwards(|entries| {
        if let Some(e) = entries.iter_mut().find(|e| e.name == name) {
            e.disabled = !e.disabled;
            found = true;
        }
    });
    if !found {
        return Err(format!("no such forward: {name}"));
    }
    cmd_list()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_tcp_spec() {
        let spec = parse_spec("8080:localhost:3000").unwrap();
        assert_eq!(
            spec,
            Spec {
                kind: Kind::Tcp,
                bind: DEFAULT_BIND.into(),
                local_port: 8080,
                host: "localhost".into(),
                host_port: 3000,
            }
        );
    }

    #[test]
    fn kind_prefixes() {
        assert_eq!(parse_spec("udp/53:r:53").unwrap().kind, Kind::Udp);
        assert_eq!(parse_spec("tcp/80:h:80").unwrap().kind, Kind::Tcp);
        assert_eq!(parse_spec("tls/8443:h:443").unwrap().kind, Kind::Tls);
        assert!(parse_spec("sctp/80:h:80").is_err());
    }

    #[test]
    fn tls_opts_become_native_options() {
        let plain = TlsOpts::default();
        assert!(!plain.insecure);
        assert!(plain.alpn.is_empty());
        let insecure = TlsOpts {
            insecure: true,
            ..TlsOpts::default()
        };
        assert!(insecure.native().insecure);
    }

    #[test]
    fn explicit_bind_address() {
        let spec = parse_spec("0.0.0.0:8080:localhost:3000").unwrap();
        assert_eq!(spec.bind, "0.0.0.0");
        assert_eq!(spec.local_port, 8080);
        assert_eq!(spec.host, "localhost");
    }

    #[test]
    fn default_bind_is_loopback() {
        // The security property, asserted rather than assumed: an unauthenticated listener must not land on a wildcard address without the operator saying so.
        assert_eq!(parse_spec("8080:h:80").unwrap().bind, "127.0.0.1");
    }

    #[test]
    fn bracketed_ipv6_bind_and_host() {
        let spec = parse_spec("[::1]:8080:[fd00::5]:3000").unwrap();
        assert_eq!(spec.bind, "::1");
        assert_eq!(spec.host, "fd00::5");
        assert_eq!(spec.local_port, 8080);
        assert_eq!(spec.host_port, 3000);
    }

    #[test]
    fn ephemeral_local_port_is_allowed() {
        assert_eq!(parse_spec("0:db.internal:5432").unwrap().local_port, 0);
    }

    #[test]
    fn zero_target_port_is_rejected() {
        // The wire refuses port 0; catching it here gives a better message than a round trip to learn the same thing.
        assert!(parse_spec("8080:host:0").is_err());
    }

    #[test]
    fn malformed_specs_are_rejected() {
        for bad in [
            "8080",
            "8080:host",
            "",
            "8080:host:notaport",
            "notaport:host:80",
            "8080::80",
            "[::1:8080:host:80",
            "[::1]x:8080:host:80",
            "1:2:8080:host:80",
        ] {
            assert!(parse_spec(bad).is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn display_round_trips() {
        for spec in [
            "8080:localhost:3000",
            "udp/5353:resolver.internal:53",
            "tls/8443:api.internal:443",
            "0.0.0.0:8080:localhost:3000",
        ] {
            let parsed = parse_spec(spec).unwrap();
            assert_eq!(parsed.to_string(), spec);
            assert_eq!(parse_spec(&parsed.to_string()).unwrap(), parsed);
        }
    }
}
