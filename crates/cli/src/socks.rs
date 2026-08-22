//! `yas socks` — a local SOCKS5 proxy over the yas connection (docs/design/net.md § Client: `yas socks`).
//!
//! `ssh -D` over any yas transport. The target comes from each accepted request
//! rather than from a spec, so one listener reaches everything the server reaches
//! and nothing has to be known in advance. That is the whole difference from
//! `yas forward`: the wire is identical, because `NET_OPEN` already names its own
//! target and the server pins nothing per connection.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::yas_net::{self, Connection, DEFAULT_BIND, OnOpen};
use yas_wire::core::Status;

// --------------------------------------------------------------------------- The SOCKS5 wire (RFC 1928) ---------------------------------------------------------------------------

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0x00;
const NO_ACCEPTABLE_METHOD: u8 = 0xff;

const CMD_CONNECT: u8 = 0x01;

const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;

const REP_OK: u8 = 0x00;
const REP_FAILURE: u8 = 0x01;
const REP_NOT_ALLOWED: u8 = 0x02;
const REP_HOST_UNREACHABLE: u8 = 0x04;
const REP_REFUSED: u8 = 0x05;
const REP_CMD_UNSUPPORTED: u8 = 0x07;
const REP_ATYP_UNSUPPORTED: u8 = 0x08;

/// Map the relay's answer onto a SOCKS reply code.
///
/// The wire keeps "did not resolve" apart from "refused" apart from "denied by
/// policy" (docs/design/net.md § Statuses), and so does SOCKS, so a client sees the
/// real reason instead of a blanket failure. Everything left over is a general
/// failure: SOCKS has no code for "the proxy ran out of stream ids".
fn reply_code(status: Status) -> u8 {
    match status {
        Status::Ok => REP_OK,
        Status::NotFound => REP_HOST_UNREACHABLE,
        Status::Io | Status::Timeout => REP_REFUSED,
        Status::Unavailable => REP_NOT_ALLOWED,
        _ => REP_FAILURE,
    }
}

/// A CONNECT reply.
///
/// BND.ADDR/BND.PORT report the address the proxy used to reach the target, which
/// only the far end of the relay knows and `NET_OPENED` does not carry. All-zero
/// IPv4 is the conventional stand-in and is what clients that ignore the field —
/// which for CONNECT is all of them — already expect.
fn connect_reply(status: Status) -> Vec<u8> {
    vec![
        VERSION,
        reply_code(status),
        0x00,
        ATYP_IPV4,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

/// A reply for a request the proxy rejects before the relay is involved.
fn refusal(code: u8) -> Vec<u8> {
    vec![VERSION, code, 0x00, ATYP_IPV4, 0, 0, 0, 0, 0, 0]
}

/// The target of a CONNECT.
#[derive(Debug, PartialEq, Eq)]
struct Request {
    /// A name or a literal, passed on unresolved — resolving on the server is half
    /// the point of a proxy whose reach is the server's.
    host: String,
    port: u16,
}

/// What went wrong before a stream was ever opened, and the reply it earns.
#[derive(Debug, PartialEq, Eq)]
enum Rejected {
    /// Not SOCKS5 at all, or a truncated handshake: nothing to answer, since a
    /// reply is only meaningful once the version is agreed.
    Unusable(String),
    /// A well-formed request this proxy will not serve.
    Refused { code: u8, why: String },
}

// --------------------------------------------------------------------------- Handshake ---------------------------------------------------------------------------

/// Pick the method from a greeting: `[ver][nmethods][methods…]`.
///
/// Only no-auth is offered. A proxy on loopback has the machine's own permissions
/// behind it and a password in front of it would be a password stored in whatever
/// pointed the client here, not a control.
fn choose_method(greeting: &[u8]) -> Result<(), Rejected> {
    if greeting.first() != Some(&VERSION) {
        let seen = greeting.first().copied().unwrap_or(0);
        return Err(Rejected::Unusable(format!(
            "not SOCKS5 (version {seen}); SOCKS4 and SOCKS4a are not supported"
        )));
    }
    if greeting.get(2..).is_some_and(|m| m.contains(&NO_AUTH)) {
        Ok(())
    } else {
        Err(Rejected::Unusable(
            "client offered no authentication method this proxy accepts".into(),
        ))
    }
}

/// Parse a request past its fixed header: `[ver][cmd][rsv][atyp][addr…][port:2]`.
fn parse_request(req: &[u8]) -> Result<Request, Rejected> {
    if req.len() < 4 {
        return Err(Rejected::Unusable("truncated request".into()));
    }
    if req[0] != VERSION {
        return Err(Rejected::Unusable("request is not SOCKS5".into()));
    }
    if req[1] != CMD_CONNECT {
        // BIND and UDP ASSOCIATE both need the server to open a listener or an
        // unconnected socket on the client's behalf, which the relay does not offer.
        return Err(Rejected::Refused {
            code: REP_CMD_UNSUPPORTED,
            why: format!("command {} is not supported (CONNECT only)", req[1]),
        });
    }
    let atyp = req[3];
    let addr = &req[4..];
    let (host, rest) = match atyp {
        ATYP_IPV4 => {
            if addr.len() < 6 {
                return Err(Rejected::Unusable("truncated IPv4 request".into()));
            }
            let ip = std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]);
            (ip.to_string(), &addr[4..])
        }
        ATYP_IPV6 => {
            if addr.len() < 18 {
                return Err(Rejected::Unusable("truncated IPv6 request".into()));
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&addr[..16]);
            (std::net::Ipv6Addr::from(octets).to_string(), &addr[16..])
        }
        ATYP_DOMAIN => {
            let Some(&len) = addr.first() else {
                return Err(Rejected::Unusable("truncated domain request".into()));
            };
            let len = len as usize;
            if addr.len() < 1 + len + 2 {
                return Err(Rejected::Unusable("truncated domain request".into()));
            }
            let name = &addr[1..1 + len];
            // The wire refuses a NUL or an over-long host with INVALID; catching it
            // here answers the client properly instead of spending a stream id on it.
            if name.is_empty() || name.contains(&0) || name.len() > yas_wire::net::MAX_HOST_BYTES {
                return Err(Rejected::Refused {
                    code: REP_FAILURE,
                    why: "unusable domain name in request".into(),
                });
            }
            let Ok(name) = std::str::from_utf8(name) else {
                return Err(Rejected::Refused {
                    code: REP_FAILURE,
                    why: "domain name is not UTF-8".into(),
                });
            };
            (name.to_string(), &addr[1 + len..])
        }
        other => {
            return Err(Rejected::Refused {
                code: REP_ATYP_UNSUPPORTED,
                why: format!("address type {other} is not supported"),
            });
        }
    };
    let port = u16::from_be_bytes([rest[0], rest[1]]);
    if port == 0 {
        // `NET_OPEN` rejects it, so answer now rather than open a stream to be told.
        return Err(Rejected::Refused {
            code: REP_FAILURE,
            why: "port 0 in request".into(),
        });
    }
    Ok(Request { host, port })
}

/// Read the greeting and the request off an accepted connection.
async fn handshake(local: &mut tokio::net::TcpStream) -> Result<Request, Rejected> {
    let truncated = |what: &str| Rejected::Unusable(format!("truncated {what}"));

    let mut head = [0u8; 2];
    local
        .read_exact(&mut head)
        .await
        .map_err(|_| truncated("greeting"))?;
    let mut methods = vec![0u8; head[1] as usize];
    local
        .read_exact(&mut methods)
        .await
        .map_err(|_| truncated("method list"))?;
    let mut greeting = head.to_vec();
    greeting.append(&mut methods);
    // The method reply is owed as soon as the version is agreed: refusing with
    // 0xff is how a client learns it offered nothing usable, rather than reading
    // an unexplained close.
    let chosen = choose_method(&greeting);
    if head[0] == VERSION {
        let method = if chosen.is_ok() {
            NO_AUTH
        } else {
            NO_ACCEPTABLE_METHOD
        };
        let _ = local.write_all(&[VERSION, method]).await;
    }
    chosen?;

    let mut head = [0u8; 4];
    local
        .read_exact(&mut head)
        .await
        .map_err(|_| truncated("request"))?;
    // The address length depends on its type, so read exactly what this one needs.
    let addr_len = match head[3] {
        ATYP_IPV4 => 4,
        ATYP_IPV6 => 16,
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            local
                .read_exact(&mut len)
                .await
                .map_err(|_| truncated("domain length"))?;
            let mut rest = vec![0u8; len[0] as usize + 2];
            local
                .read_exact(&mut rest)
                .await
                .map_err(|_| truncated("domain request"))?;
            let mut req = head.to_vec();
            req.extend_from_slice(&len);
            req.append(&mut rest);
            return parse_request(&req);
        }
        // An unknown address type has no length, so the request cannot be read
        // past this point and the connection ends with the reply.
        _ => return parse_request(&head),
    };
    let mut rest = vec![0u8; addr_len + 2];
    local
        .read_exact(&mut rest)
        .await
        .map_err(|_| truncated("request address"))?;
    let mut req = head.to_vec();
    req.append(&mut rest);
    parse_request(&req)
}

// --------------------------------------------------------------------------- Listen address ---------------------------------------------------------------------------

/// Where to listen: `[bind_address:]port`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listen {
    pub bind: String,
    pub port: u16,
}

impl Listen {
    /// The bound address, with an IPv6 literal bracketed so it parses as one.
    fn addr(&self) -> String {
        format!("{}:{}", yas_net::bracket(&self.bind), self.port)
    }
}

/// Parse `[bind_address:]port`. An IPv6 bind address must be bracketed, since
/// otherwise its own colons are indistinguishable from the port separator.
pub fn parse_listen(s: &str) -> Result<Listen, String> {
    let bad = |what: &str| Err(format!("{s}: {what}"));
    let (bind, port) = if let Some(rest) = s.strip_prefix('[') {
        match rest.split_once(']') {
            Some((addr, port)) => match port.strip_prefix(':') {
                Some(port) => (addr.to_string(), port),
                None => return bad("want [bind_address:]port"),
            },
            None => return bad("unterminated [address]"),
        }
    } else if let Some((bind, port)) = s.rsplit_once(':') {
        (bind.to_string(), port)
    } else {
        (DEFAULT_BIND.to_string(), s)
    };
    if bind.is_empty() {
        return bad("empty bind address");
    }
    // Port 0 means "pick one and tell me", as it does for a forward.
    let Ok(port) = port.parse::<u16>() else {
        return bad(&format!("bad port `{port}`"));
    };
    Ok(Listen { bind, port })
}

// --------------------------------------------------------------------------- Entry point ---------------------------------------------------------------------------

pub async fn cmd_socks(on: Option<&str>, hub: &str, listen: Listen) -> Result<i32, String> {
    // Bind before the connection is spent, so a busy port fails against nothing.
    let addr = listen.addr();
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("cannot bind {addr}: {e}"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("cannot read local address: {e}"))?;

    let conn = Connection::connect(on, hub).await?;
    eprintln!("yas: SOCKS5 proxy on {local} → the server's network");
    let server_connection = conn.clone();
    tokio::spawn(async move { serve(listener, server_connection).await });

    // The reader owns the rest of the process's life: when the connection drops,
    // every proxied socket goes with it.
    conn.wait_closed().await;
    Ok(0)
}
async fn serve(listener: tokio::net::TcpListener, conn: Connection) {
    loop {
        let (local, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("yas: accept failed on the SOCKS5 listener: {e}");
                return;
            }
        };
        let conn = conn.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy(local, conn).await {
                eprintln!("yas: {peer}: {e}");
            }
        });
    }
}

async fn proxy(mut local: tokio::net::TcpStream, conn: Connection) -> Result<(), String> {
    let request = match handshake(&mut local).await {
        Ok(request) => request,
        Err(Rejected::Unusable(why)) => return Err(why),
        Err(Rejected::Refused { code, why }) => {
            let _ = local.write_all(&refusal(code)).await;
            return Err(why);
        }
    };
    yas_net::relay_tcp(
        local,
        conn,
        request.host,
        request.port,
        None,
        OnOpen::Answer(connect_reply),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(bytes: &[u8]) -> Result<Request, Rejected> {
        parse_request(bytes)
    }

    #[test]
    fn a_truncated_request_is_unusable_rather_than_a_panic() {
        assert!(matches!(request(&[5, 1]), Err(Rejected::Unusable(_))));
        assert!(matches!(request(&[]), Err(Rejected::Unusable(_))));
        assert!(matches!(choose_method(&[5]), Err(Rejected::Unusable(_))));
    }

    #[test]
    fn ipv4_request() {
        let got = request(&[5, 1, 0, ATYP_IPV4, 93, 184, 216, 34, 0x01, 0xbb]).unwrap();
        assert_eq!(
            got,
            Request {
                host: "93.184.216.34".into(),
                port: 443
            }
        );
    }

    #[test]
    fn ipv6_request() {
        let mut bytes = vec![5, 1, 0, ATYP_IPV6];
        bytes.extend_from_slice(&[0u8; 15]);
        bytes.push(1);
        bytes.extend_from_slice(&80u16.to_be_bytes());
        assert_eq!(
            request(&bytes).unwrap(),
            Request {
                host: "::1".into(),
                port: 80
            }
        );
    }

    /// A name must reach the wire unresolved: the server resolves it, which is how
    /// a proxied client reaches names it cannot look up itself.
    #[test]
    fn domain_request_is_not_resolved() {
        let mut bytes = vec![5, 1, 0, ATYP_DOMAIN, 11];
        bytes.extend_from_slice(b"example.com");
        bytes.extend_from_slice(&443u16.to_be_bytes());
        assert_eq!(
            request(&bytes).unwrap(),
            Request {
                host: "example.com".into(),
                port: 443
            }
        );
    }

    #[test]
    fn unsupported_commands_and_address_types_are_refused_by_code() {
        let bind = request(&[5, 0x02, 0, ATYP_IPV4, 127, 0, 0, 1, 0, 80]).unwrap_err();
        assert_eq!(
            bind,
            Rejected::Refused {
                code: REP_CMD_UNSUPPORTED,
                why: "command 2 is not supported (CONNECT only)".into()
            }
        );
        let atyp = request(&[5, 1, 0, 0x09, 0, 0]).unwrap_err();
        assert!(matches!(
            atyp,
            Rejected::Refused {
                code: REP_ATYP_UNSUPPORTED,
                ..
            }
        ));
    }

    /// Both are `INVALID` on the wire; answering here saves a stream id and gives
    /// the client a reply instead of a close.
    #[test]
    fn requests_the_wire_would_reject_are_refused_locally() {
        for bytes in [
            vec![5, 1, 0, ATYP_IPV4, 127, 0, 0, 1, 0, 0],
            vec![5, 1, 0, ATYP_DOMAIN, 1, 0, 0, 80],
        ] {
            assert!(matches!(
                request(&bytes).unwrap_err(),
                Rejected::Refused { .. }
            ));
        }
    }

    #[test]
    fn socks4_is_named_rather_than_dropped() {
        let err = choose_method(&[4, 1, 0]).unwrap_err();
        let Rejected::Unusable(why) = err else {
            panic!("SOCKS4 must be unusable, not refused");
        };
        assert!(why.contains("SOCKS4"), "{why}");
    }

    #[test]
    fn only_no_auth_is_accepted() {
        assert!(choose_method(&[5, 1, NO_AUTH]).is_ok());
        assert!(choose_method(&[5, 2, 0x02, NO_AUTH]).is_ok());
        assert!(choose_method(&[5, 1, 0x02]).is_err());
    }

    /// The point of the mapping: a client can tell a DNS failure from a refusal
    /// from a policy denial, which a blanket 0x01 would flatten.
    #[test]
    fn statuses_map_to_distinct_socks_replies() {
        assert_eq!(reply_code(Status::Ok), REP_OK);
        assert_eq!(reply_code(Status::NotFound), REP_HOST_UNREACHABLE);
        assert_eq!(reply_code(Status::Io), REP_REFUSED);
        assert_eq!(reply_code(Status::Unavailable), REP_NOT_ALLOWED);
        assert_eq!(reply_code(Status::ResourceExhausted), REP_FAILURE);
        assert_eq!(reply_code(Status::Internal), REP_FAILURE);
    }

    #[test]
    fn connect_reply_is_ten_bytes_of_ipv4_shaped_reply() {
        let ok = connect_reply(Status::Ok);
        assert_eq!(ok, vec![5, 0, 0, ATYP_IPV4, 0, 0, 0, 0, 0, 0]);
        assert_eq!(connect_reply(Status::Io)[1], REP_REFUSED);
    }

    #[test]
    fn listen_defaults_to_loopback() {
        assert_eq!(
            parse_listen("1080").unwrap(),
            Listen {
                bind: DEFAULT_BIND.into(),
                port: 1080
            }
        );
    }

    #[test]
    fn explicit_bind_addresses() {
        assert_eq!(
            parse_listen("0.0.0.0:1080").unwrap(),
            Listen {
                bind: "0.0.0.0".into(),
                port: 1080
            }
        );
        let v6 = parse_listen("[::1]:1080").unwrap();
        assert_eq!(
            v6,
            Listen {
                bind: "::1".into(),
                port: 1080
            }
        );
        // Bracketed again on the way out, or it would not parse as one address.
        assert_eq!(v6.addr(), "[::1]:1080");
    }

    #[test]
    fn ephemeral_port_is_allowed() {
        assert_eq!(parse_listen("0").unwrap().port, 0);
    }

    #[test]
    fn malformed_listen_addresses_are_rejected() {
        for s in [
            "",
            "http",
            "1080x",
            "65536",
            "[::1:1080",
            "[::1]1080",
            ":1080",
        ] {
            assert!(parse_listen(s).is_err(), "accepted {s:?}");
        }
    }
}
