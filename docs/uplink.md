# Uplink — exposing a YAS server through an untrusted relay

`yas uplink <control-url> --allow-client PUBLIC_KEY`, with the private key in
`YAS_UPLINK_IDENTITY`, makes
the local YAS server reachable outside NAT. It holds an outbound WebTransport
session to a relay. Each relay-initiated stream must complete end-to-end
mutual authentication before it can reach the local server socket. This document
specifies the protocol between the uplink and its control endpoint and
relay. It leaves the relay side abstract: a WebTransport server that opens
one bidirectional stream per consumer and
forwards opaque bytes can act as a relay. The inner stream carries TLS records,
not a plaintext YAS preface.

## Roles

| Role             | Meaning                                                              |
| ---------------- | -------------------------------------------------------------------- |
| uplink           | `yas uplink` — connects out, bridges streams to the local YAS server |
| control endpoint | HTTPS URL that authenticates the uplink and allocates it a relay     |
| relay            | WebTransport server the uplink stays connected to                    |
| consumer         | A YAS client reaching the server through the relay                   |

## End-to-end trust and setup

The consumer pins the producer's Ed25519 public key. The producer loads its
own Ed25519 private key and an explicit allowlist of client public keys.
Exchange public keys through a trusted channel, independently of the relay
and control plane. Neither endpoint accepts keys from allocation responses.
There is no trust-on-first-use, bearer-only admission, or plaintext fallback.

Generate a separate identity once on each endpoint and print its public key:

```bash
export YAS_UPLINK_IDENTITY="$(yas uplink-keygen --private)"
yas uplink-public-key
```

`uplink-keygen --private` prints only the private seed, suitable for shell
capture. `uplink-public-key` derives the public key from `YAS_UPLINK_IDENTITY`.
Keep the generated private key in your secret configuration and reload it on
later starts; generating a replacement changes the public key others must trust.
One client identity can be authorized on multiple YAS servers.

For scripts that need both values, `yas uplink-keygen` still prints JSON with
`private_key` and `public_key`. Both are exactly 43 characters of canonical,
unpadded base64url: the private value encodes a 32-byte Ed25519 seed, and the
public value encodes the 32-byte public key.
There is no key file or external DER wrapper. Padding, standard-base64 `+`/`/`,
noncanonical trailing bits, and wrong lengths are rejected.

With the producer's own private key in its environment, authorize the client's
public key and start the uplink:

```bash
export YAS_UPLINK_TOKEN=CONTROL_TOKEN
yas uplink https://relay.example --allow-client CLIENT_PUBLIC_KEY
```

`YAS_UPLINK_IDENTITY` is the recommended private-key input so it stays out of
process arguments. `--identity PRIVATE_KEY` is an explicit alternative.
`--allow-client` is repeatable; `YAS_UPLINK_CLIENT_KEYS` accepts comma-separated
base64url public keys. An empty allowlist is rejected before contacting the
control endpoint. Keys and the producer allowlist are loaded at startup; apply
rotation or revocation by restarting the uplink with the new configuration.
Existing streams retain their authenticated authority until disconnected.

To generate a connection URL, run this on the producer with its private key
in `YAS_UPLINK_IDENTITY` and a consumer routing token from the relay service:

```bash
export YAS_UPLINK_CLIENT_TOKEN=CLIENT_TOKEN
yas uplink-url https://relay.example
```

`--client-token CLIENT_TOKEN` overrides `YAS_UPLINK_CLIENT_TOKEN`. The producer's
`YAS_UPLINK_TOKEN` is a separate credential. The command works offline, derives
the server pin locally, and prints a URL containing exactly the consumer token
and server public key. Token punctuation is encoded automatically. Transfer
this URL to the client through a trusted channel. The client's public key must
be in the producer's allowlist.

Successful consumers receive full YAS authority as the server's OS identity.
The relay can observe endpoints, traffic sizes, timing, and routing tokens;
it can deny service, but cannot decrypt YAS payloads, inject commands, or
impersonate an allowed client. A compromised control plane can redirect the
outer transport, but the independent producer pin still rejects impersonation.
The consumer software and its host must remain trusted.

## Inner TLS protocol

Every consumer stream starts with TLS 1.3, using RFC 7250 raw public keys
encoded as RFC 8410 Ed25519 SubjectPublicKeyInfo. Both sides require Ed25519
CertificateVerify signatures. The only key exchange group is ephemeral
X25519, and the cipher suite is TLS_CHACHA20_POLY1305_SHA256. No X.509 CA,
DNS name, certificate expiry, or outer TLS certificate confers inner trust.
ALPN is exactly `yas-uplink/1`; resumption, tickets, and early data are disabled.
Rustls handles the TLS handshake, transcript signatures, key schedule, records,
and key updates.

After verifying the client's proof of possession, the producer sends the
11 encrypted bytes `YAS-UPLINK\x01` and flushes. The consumer checks this
confirmation before releasing the connection to YAS: TLS 1.3's handshake
alone does not give the client a final acknowledgment of client authentication.
The producer then accepts an encrypted YAS preface or composite-main selector.
It opens no local socket before authentication and selector validation succeed.

Authentication has a 10-second timeout and at most 64 pending streams per
relay session. Authenticated selector parsing has a further 5-second timeout.
Invalid keys, signatures, versions, ALPN, and legacy plaintext streams close
without reaching local IPC. Clean TLS close-notify propagates as a half-close;
a TLS integrity error terminates the bridge and its optional sideband.

## Control endpoint

The uplink authenticates to the control endpoint with the
`YAS_UPLINK_TOKEN` environment variable:

```
GET <control-url>
Authorization: Bearer <YAS_UPLINK_TOKEN>
Accept: application/json
```

A success response is the **relay pool**:

```json
{ "relays": ["https://relay-1.example.com:4443/t/kfV3aB#sha256=<base64url>"] }
```

- `relays` is a non-empty array of `https` URLs. Any other scheme is an
  error.
- **A relay URL is a credential.** Whatever authenticates the uplink to
  the relay (a token in the path, a capability URL) is embedded in it.
  Implementations MUST NOT log relay URLs; log `host:port` instead.
- An optional URL fragment `#sha256=<base64url SHA-256>` pins the relay's
  TLS certificate (32 bytes, DER hash of the end-entity certificate).
  With a pin, chain and expiry are not checked — the hash is the trust
  anchor, exactly like the browser's `serverCertificateHashes`. Without
  one, system roots verify as usual. A malformed pin is an error, never a
  silent fall-back to system roots. The fragment is client-side only and
  is stripped before connecting.
- Unknown fields in the response are ignored.

Error handling:

- `401`/`403` — the token is bad; fatal, the uplink exits.
- Any other failure (unreachable, non-2xx, malformed body) — retried
  with exponential backoff, 1s doubling to a 60s cap, with 0.75×–1.25×
  jitter. A `Retry-After` header (seconds) overrides the backoff delay.

## Consumer attachment

A consumer URI carries URL-form-encoded routing and server-pin fields:

```text
uplink:https://relay.example#token=CLIENT_TOKEN&server=SERVER_PUBLIC_KEY
```

`token` and `server` are mandatory. `server` is the producer's 43-character
base64url public key. The private identity comes from `YAS_UPLINK_IDENTITY` in
the connecting process's environment. An optional `identity=CLIENT_PRIVATE_KEY`
fragment field overrides that environment value. Unknown, duplicate, empty,
or malformed fields are rejected. Encode token punctuation such as `+`, `&`,
`#`, and spaces with percent encoding; base64url keys need no escaping.

For a home server's Relay route, the default private key comes from that home
server's environment. For a CLI connection, the CLI passes its current
environment key to an existing proxy over authenticated local IPC. It does
not add the key to process arguments or persist it in the saved remote URI.
An explicitly configured identity is a full-control secret; neither form is
sent to the relay or control plane.

The control URL must be HTTPS without userinfo or query parameters. The client
requests `<control-path>/attach` with only `token` in the Authorization header.
The fragment, server pin, and private identity are never included in that request.
The client disables HTTP redirects. `/attach` waits for the uplink and returns:

```json
{ "ws": "wss://relay-worker.example/u/session" }
```

The client connects to this WSS worker using the existing bearer-token/`ok`
exchange, then starts inner TLS over its binary byte stream. WebSocket messages
are opaque chunks of at most 64 KiB; their boundaries have no inner meaning.
The built-in consumer sends chunks of at most 16 KiB. Worker allocation
and bearer authentication provide routing only. The pinned server and allowed
client keys determine access to YAS. Worker URLs and errors are not logged.

On the client, with its own original private key in `YAS_UPLINK_IDENTITY`, save
the URL printed by the producer:

```bash
yas remote add sandbox 'UPLINK_URL_FROM_PRODUCER'
yas --on sandbox terminal list
```

This works through the native proxy, with `YAS_PROXY=0`, and through a home
server's Relay connector. `yas remote list` masks the fragment unless passed
`--reveal`. Tokens remain credentials for the control/worker service, but
possession of one without an allowed private key does not grant YAS access.

Both producers and consumers must upgrade and configure keys. Old
`uplink:https://relay.example#CLIENT_TOKEN` URIs and unencrypted consumer
streams are intentionally rejected. Relays must forward inner TLS unchanged.

## Relay session

The uplink shuffles the pool and tries each relay in order: a
WebTransport (HTTP/3 CONNECT) session to the relay URL. Liveness settings
are a **10s keepalive** and a **30s idle timeout**, so a dead relay is
noticed within 30 seconds without any application-level pings.

The uplink never opens streams. The relay opens **one bidirectional stream
per consumer**. After inner TLS authentication, the uplink bridges decrypted
bytes to a fresh local YAS socket. Direct streams carry the normal YAS preface
and length-prefixed frames, unparsed and unreframed inside TLS.

### Encrypted native datagrams

A WebTransport consumer can send a composite-main selector **inside inner
TLS**. Its random 16-byte route token identifies the optional datagram lane.
Selectors outside TLS are rejected. The consumer must implement this inner
protocol; the built-in `uplink:` connector uses the reliable WebSocket lane.

Both endpoints export 64 bytes from the established TLS connection with label
`EXPORTER-YAS-UPLINK-v1-datagrams` and an empty context. Bytes 0–31 are the
client-to-producer ChaCha20-Poly1305 key; bytes 32–63 are the reverse key.
Keys are unique to the inner session and independent of relay TLS keys.
Each routed packet is:

```text
route token (16 bytes) | sequence (8 bytes, big-endian) | ciphertext | tag (16 bytes)
```

The nonce is four zero bytes followed by the sequence. AAD is the token
followed by the sequence. Each direction starts its counter at zero, increments
it per packet, and refuses to wrap. Receivers authenticate before updating a
128-packet replay window, permit reordering within that window, and discard
forgeries, duplicates, and older packets. A token change, direction reflection,
or packet from another session fails authentication.

The advertised plaintext datagram maximum must leave room for the 16-byte
routing token and 24-byte encryption overhead within the physical path MTU.
Routes remain bounded and lossy. Invalid datagrams are dropped without closing
the reliable stream; there is no unencrypted or reliable-lane fallback for
failed ciphertext authentication.

### Closure

Failed authentication or unavailable local IPC closes the consumer stream.
On SIGINT the outer session closes with application code 2. The worker should
close its consumer connection when the corresponding stream closes.

## Reconnection

- A relay that never accepted the session (handshake or CONNECT failed):
  try the next relay in the shuffled pool.
- A session that was actually established and later died: re-query the
  control endpoint immediately for a fresh pool — allocation is
  re-balanced on every reconnect — and reset the backoff.
- Pool exhausted with no session established: back off (same schedule as
  the control endpoint) and re-query.
- On SIGINT the uplink closes the active session with code 2 instead of
  letting it idle out on the relay.

Relay URLs stay valid for as long as their embedded credential does; the
uplink treats each pool response as single-use and re-queries rather than
caching it.
