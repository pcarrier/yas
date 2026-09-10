# Uplink — exposing a YAS server through an untrusted relay

`yas uplink <control-url> --allow-client PUBLIC_KEY`, with the private key in
`YAS_UPLINK_IDENTITY`, makes
the local YAS server reachable outside NAT. It holds an outbound WebTransport
session to a relay. Each relay-initiated stream must complete end-to-end
mutual authentication before it can reach the local server socket. This document
specifies the protocol between the uplink and its control endpoint and
relay. It leaves the relay side abstract: a WebTransport server that opens
one bidirectional stream per consumer and
forwards opaque bytes can act as a relay. The inner stream carries Noise records.

## Roles

| Role             | Meaning                                                              |
| ---------------- | -------------------------------------------------------------------- |
| uplink           | `yas uplink` — connects out, bridges streams to the local YAS server |
| control endpoint | HTTPS URL that authenticates the uplink and allocates it a relay     |
| relay            | WebTransport server the uplink stays connected to                    |
| consumer         | A YAS client reaching the server through the relay                   |

## End-to-end trust and setup

The consumer pins the producer's X25519 public key. The producer loads its
own X25519 private key and an explicit allowlist of client public keys.
Exchange public keys through a trusted channel, independently of the relay
and control plane. Neither endpoint accepts keys from allocation responses.
There is no trust-on-first-use, bearer-only admission, or plaintext fallback.

Generate a separate identity once on each endpoint and print its public key:

```bash
export YAS_UPLINK_IDENTITY="$(yas uplink-keygen --private)"
yas uplink-public-key
```

`uplink-keygen --private` prints only the private key, suitable for shell
capture. `uplink-public-key` derives the public key from `YAS_UPLINK_IDENTITY`.
Keep the generated private key in your secret configuration and reload it on
later starts; generating a replacement changes the public key others must trust.
One client identity can be authorized on multiple YAS servers.

For scripts that need both values, `yas uplink-keygen` still prints JSON with
`private_key` and `public_key`. Both are exactly 43 characters of canonical,
unpadded base64url: the private value encodes a 32-byte X25519 private key, and the
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

## Inner Noise protocol (version 2)

Every consumer stream uses `Noise_IK_25519_AESGCM_SHA256` with the exact
11-byte prologue `YAS-UPLINK\x02`. X25519 supplies static identity and fresh
ephemeral key agreement; AES-256-GCM encrypts messages; SHA-256 and HKDF-SHA256
bind the transcript and derive keys. There is no algorithm negotiation,
resumption, or early application data. The consumer is the Noise initiator,
with the producer's static public key pinned before connecting.

All handshake and transport messages have a two-byte **big-endian** ciphertext
length prefix. The first IK message has 96 bytes and the response 48 bytes;
both have empty handshake payloads. Snow implements native Noise. The browser
uses WebCrypto and is checked against the same upstream Cacophony vectors and
a live native endpoint. Outer HTTPS, WSS, and WebTransport retain their TLS;
outer certificates and bearer tokens grant no inner authority.

Transport plaintext is a one-byte record type followed by content:

| Type | Content       | Meaning                      |
| ---- | ------------- | ---------------------------- |
| `0`  | 1–16384 bytes | Reliable stream data         |
| `1`  | Empty         | Authenticated write-side FIN |

Each ciphertext has a 16-byte authentication tag. Ciphertext lengths outside
17–16401 bytes, unknown record types, and empty data records are rejected.
Each direction uses Noise's independent key and implicit 64-bit counter.
After every `2^20` transport messages (including confirmation and FIN), that
direction performs the standard Noise `Rekey` operation. Counters do not reset;
nonce exhaustion closes the stream. This bounds AES-GCM usage per key.

After IK, the consumer sends `YAS-UPLINK\x02` inside a data record. The producer
checks this fresh-session proof **before opening any local socket**: IK's
first message can be replayed, but this confirmation depends on the producer's
fresh ephemeral key. The producer then generates 64 random datagram root-key
bytes and returns `YAS-UPLINK\x02` followed by those bytes in an encrypted data
record. The consumer verifies it before releasing the connection to YAS.
The first 32 root-key bytes are client-to-producer; the rest are the reverse.
The public Noise handshake hash is never used as secret key material.

The producer next validates the encrypted YAS preface or composite-main
selector, then connects to local IPC. Authentication has a 10-second timeout
and at most 64 pending streams per relay session. Selector parsing has a
further 5-second timeout. Keys, transcript tampering, missing confirmation,
wrong versions, and legacy plaintext/TLS streams fail closed.

Authenticated FIN propagates as a half-close, allowing the reverse direction
to finish. Carrier EOF without FIN is a truncation error. Integrity errors
terminate both bridge directions and their optional datagrams. Buffers and
browser asynchronous queues are bounded; saturation fails the reliable lane
rather than silently dropping reliable bytes.

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
exchange, then starts Noise over its binary byte stream. WebSocket messages
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
streams are intentionally rejected. Relays must forward Noise bytes unchanged.

## Relay session

The uplink shuffles the pool and tries each relay in order: a
WebTransport (HTTP/3 CONNECT) session to the relay URL. Liveness settings
are a **10s keepalive** and a **30s idle timeout**, so a dead relay is
noticed within 30 seconds without any application-level pings.

The uplink never opens streams. The relay opens **one bidirectional stream
per consumer**. After Noise authentication, the uplink bridges decrypted
bytes to a fresh local YAS socket. Direct streams carry the normal YAS preface
and length-prefixed frames, unparsed and unreframed inside Noise.

### Encrypted native datagrams

A consumer with an unreliable carrier sends a composite-main selector **inside
Noise**. Its random nonzero 16-byte route token identifies the optional datagram
lane. The native and direct browser `uplink:` connectors use the reliable WSS
lane. Browser embedders can wrap an opaque carrier exposing routed datagrams
with `YasNoiseTransport`; it sends the selector and encrypts datagrams itself.

Each routed packet is:

```text
route token (16 bytes) | counter (8 bytes, big-endian) | ciphertext | tag (16 bytes)
```

The independent datagram root keys are delivered in the authenticated Noise
confirmation. For each direction, derive the AES-256-GCM key as:

```text
epoch = floor(counter / 2^20)
key = HKDF-SHA256(root, salt=route_token,
                 info="YAS-UPLINK-v2-datagram" || direction_u8 || epoch_u64_be,
                 length=32)
```

Direction is 0 for client-to-producer and 1 for producer-to-client. The nonce
is four zero bytes followed by the counter. AAD is the route token followed
by the counter. Keys are separate from reliable Noise record keys. Each
sender starts at zero, increments before encryption, and refuses to wrap.
A reconnect establishes fresh roots. Loss cannot desynchronize rotation:
the counter identifies the correct epoch without a key-update datagram.

Receivers authenticate before updating a 128-packet replay window or cached
epoch key. They permit reordering across epochs within that window and discard
forgeries, duplicates, and older packets. Browser receivers recheck the window
after asynchronous decryption so concurrent duplicates cannot both pass.
The browser caps in-flight datagram crypto operations at 64 per direction.
Route changes, direction reflection, and cross-session packets fail authentication.

The advertised plaintext maximum subtracts the 16-byte routing token and
24-byte encryption overhead from the physical datagram MTU. Routes remain
bounded and lossy. Invalid packets are dropped without closing the reliable
stream or falling back to plaintext or reliable delivery. Packet loss never
prevents decrypting the next packet.

Audio codecs still require media-level ordering, playback deadlines, and loss
handling. The native browser Media output handler currently expects ordered
frames; this transport does not change its delivery policy or automatically
move audio onto unreliable datagrams.

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

## Browser embedding

Serve the browser client from an origin trusted independently of the relay.
A relay that can replace client JavaScript can steal or use its keys. The
control endpoint must allow that trusted origin through CORS, including the
Authorization header; redirects are rejected. Browser WebCrypto must support
X25519, AES-GCM, SHA-256, HMAC, and HKDF. No TLS or crypto WASM is needed.

```ts
import {
  generateUplinkKeyPair,
  YasUplinkTransport,
} from "@yas-run/core/transports";

const keys = await generateUplinkKeyPair(); // Generate once and retain securely.
// Authorize keys.publicKey on the producer, then load its pinned connection URL.
const transport = new YasUplinkTransport(connectionUrl, keys.privateKey);
// Pass transport to YasConnection, or register listeners before connect().
```

`uplinkPublicKey(privateKey)` derives the public key. `YasWorkspace` also
accepts `{ type: "uplink", url: connectionUrl, identity: keys.privateKey }`.
The library retains the identity in memory and does not save it in localStorage
or send it to the relay. Import/storage UI belongs to the embedding application.
The existing YAS UI's home-server Relay routes continue to use the home server
as the trusted client endpoint.

For a custom carrier, use
`new YasNoiseTransport(carrier, privateKey, serverPublicKey)`. Its carrier must
expose an opaque reliable byte stream and signal `connected` only after relay
routing succeeds. Optional carrier datagrams contain the 16-byte routing token
followed by encrypted packets. Advertise their complete physical maximum via
`maxDatagramSize`. The wrapper advertises only authenticated plaintext capacity
and requires the native composite selector on the producer.

## Migration from inner TLS

This version intentionally changes the wire protocol and identity algorithm.
Generate X25519 identities, update producer allowlists, and redistribute pinned
connection URLs. Ed25519 public pins are not interchangeable with X25519 pins,
even though both encodings have 43 characters. There is no silent protocol
fallback. Flags, environment variable names, and URI fragment field names stay
the same; private keys still work through `YAS_UPLINK_IDENTITY` without appearing
in process arguments. Both endpoints must upgrade together.
