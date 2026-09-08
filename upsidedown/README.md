# upsidedown

Hosted relay for `blit uplink` — exposes NATed blit servers to consumers
through a Fly.io app. Design: [docs/upsidedown.md](../docs/upsidedown.md).

One app, four process groups (`fly.toml`): a control plane on `443` and
three workers each owning one external port for TCP (WebSocket) and UDP
(WebTransport). The binary (`crates/upsidedown`) selects its role from the
subcommand.

## First-time setup

```bash
# App + a dedicated IPv4 (required for UDP / WebTransport).
flyctl apps create blit-upsidedown --org <org>
flyctl ips allocate-v4 -a blit-upsidedown
flyctl ips allocate-v6 -a blit-upsidedown

# Managed Redis, then point the app at it.
flyctl redis create --org <org>            # prints a redis:// URL
flyctl secrets set -a blit-upsidedown \
  REDIS_URL='redis://…' \
  UPSIDEDOWN_PUBLIC_KEYS='<base64url ed25519 pubkey>[,<pubkey>…]' \
  UPSIDEDOWN_SEAL_KEY="$(head -c32 /dev/urandom | basenc --base64url | tr -d '=')"

# DNS: point usd.blit.sh at the app's IPs (ACME issues the certificate).
flyctl ips list -a blit-upsidedown
```

`UPSIDEDOWN_SEAL_KEY` (32 random bytes, base64url) seals the ACME
certificate and account key in Redis; keep it stable or the stored cert
becomes unreadable and is re-issued.

## Deploy

```bash
nix run .#deploy-upsidedown        # flyctl deploy, repo root as build context
```

CD runs the same task on push to `main` touching `upsidedown/**` or
`crates/upsidedown/**` (`.github/workflows/deploy-upsidedown.yml`), using
the `FLY_API_TOKEN` secret.

## Tokens

Signing keys live with the operator (e.g. the indent.com backend); the app
only holds the public keys. Generate a keypair and mint tokens with the
same binary:

```bash
upsidedown keygen
# → put `public:` in UPSIDEDOWN_PUBLIC_KEYS, keep `secret:` off the app

upsidedown mint --secret-key <secret> --sid <session> --role server --ttl-secs 31536000
upsidedown mint --secret-key <secret> --sid <session> --role client  --ttl-secs 600
```

- `server` token → `BLIT_UPLINK_TOKEN`, used by `blit uplink https://usd.blit.sh/pool`.
- `client` token → any blit client via `uplink:<jwt>` (default control
  plane `https://usd.blit.sh`), e.g. `blit remote add sandbox uplink:<jwt>`.

## Uplink diagnostics

Clients and workers emit JSON lifecycle events and cumulative QUIC statistics
every 60 seconds. Correlate by `connection_id` (new for every connection), then
`sid`. Client logs identify the relay by host/port; worker logs identify `worker`.
Records include client version, elapsed milliseconds, RTT, sent/lost packet
counts, received datagrams, sent/received UDP bytes, and close reason. Counters
include transport overhead, not just relayed application bytes. A client-side
`cancelled` event means the local task was dropped, including on Ctrl-C. Abrupt
process termination cannot emit a final event.

The client sends optional `x-blit-connection-id` and `x-blit-version` CONNECT
headers. Older peers remain compatible; older clients appear as version
`unknown` and get a worker-generated connection ID. Client-side `sid` is decoded
from the credential only for diagnostics, not verified or used for authorization.
JWTs, CONNECT URLs, and bearer headers are never intentionally logged.

Worker `transport_connected` means QUIC/WebTransport was accepted; `registered`
means the Redis session binding was written successfully. `registration_failed`
closes the session with code 503 so the client reconnects through the pool.
`session_publish_failed` leaves the binding intact but warns that attach waiters
may miss the notification. `session_refresh_failed` means Redis could not refresh
the binding; `session_refresh_not_owner` means it expired or another worker owns
it. Existing connections continue during refresh failures; no binding is
overwritten. `session_delete_failed` reports failed teardown cleanup (TTL remains
the backstop).

## Local development

```bash
REDIS_URL=memory:// UPSIDEDOWN_HTTP=1 \
UPSIDEDOWN_HOST=127.0.0.1 UPSIDEDOWN_LISTEN=127.0.0.1:8443 \
UPSIDEDOWN_PUBLIC_KEYS=<pubkey> UPSIDEDOWN_SEAL_KEY=<32B base64url> \
  cargo run -p blit-upsidedown -- dev --workers 2
```

`dev` runs the control plane and workers in one process against an
in-memory store. `UPSIDEDOWN_HTTP=1` serves plain HTTP (no TLS); without an
ACME domain and without it, the control plane serves a self-signed
certificate and `/pool` pins it. Set `UPSIDEDOWN_ACME_STAGING=1` to use
Let's Encrypt staging while wiring up a real domain.
