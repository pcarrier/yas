# The server can reach things the client cannot

- Open TCP, UDP, Unix stream/datagram/seqpacket, or Windows byte/message pipes; add TLS/SNI/ALPN when needed.
- Keep application protocols in client code: HTTP, DNS, Postgres, WebSocket.
- `yas forward`: `ssh -L`-style lists plus UDP; save forwards for later.
- `yas socks`: `ssh -D`-style SOCKS5; resolve names on the server side.
- Choose native-required, native-preferred, or an explicit reliable-tunnel fallback; report sequence/drops and never fragment silently.
- Enforce destination allow policy; bind listeners to loopback by default.
