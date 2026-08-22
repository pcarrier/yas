# RFC: Server Environment

- **Status:** Implemented as native YAS Environment family v1
- **Date:** 2026-08-18
- **Companion to:** [processes.md](processes.md), [extensions.md](extensions.md),
  [kv.md](kv.md), [../protocol.md](../protocol.md)

## Summary

One request, one reply: a client asks for the yas server's environment and
receives every variable, sorted by key.

Two derived entries are merged over the process environment without mutating
it. `YAS_SERVER_NAME` is the effective name (`default` when no override was
supplied). `YAS_SOCKET_TEMPLATE` is the canonical automatic YAS endpoint with
one literal `{name}` placeholder, computed by the same owner-private runtime
resolver used for server bind. It predicts automatic named endpoints only; an
explicit `YAS_SOCK` for this or another server never changes the template.
These entries give built-in extensions the same instance identity and socket
resolution as the host.

It exists because **a client has no other way to learn anything about the
session it is attached to.** Before this, `WAYLAND_DISPLAY` appeared exactly once
in the whole server — inside a dead function — and no message carried the
compositor socket, `XDG_RUNTIME_DIR`, the desktop bus address, or the audio
sockets. That knowledge belonged solely to the PTY spawn path.

The immediate consumer is an extension. Wasm's five-import host ABI and
QuickJS's native bindings both expose the same YAS session, with no direct
filesystem, process, or environment access; everything else an extension does,
it does by speaking this protocol as an ordinary client. So an extension that
wants to enumerate installed applications cannot read `XDG_DATA_DIRS` to find
them. It can already _read_ `/usr/share/applications` through the fs family,
which accepts an arbitrary root. It just could not find out where to look.

Putting this in the protocol rather than adding a sixth wasm import is
deliberate: the ABI is kept minimal on purpose, and a native family gets
HELLO selection, a dispatch-level kill switch, and reach for every client
rather than extensions alone.

## Native YAS contract

Environment is family `0x0045`, version 1. `GET` is its only Request. The
canonical schema is [`protocol/yas/families/env.toml`](../../protocol/yas/families/env.toml)
and the family-level behavior is specified in [yas.md](yas.md#environment-family).

The Result contains entries sorted by raw byte key, so an unchanged
environment has deterministic record order. Keys and values are **raw bytes,
not UTF-8**: a Unix environment carries no such guarantee, and dropping an
entry that failed to decode would be a worse answer than handing it over as it
is. Results up to 32 KiB are inline; larger answers use a bounded MESSAGE
Transfer.

A NUL in either half is `INVALID` — it cannot survive `execve`, so the codec
refuses to claim it round-tripped. A duplicate key is `INVALID` rather than
merged, since either resolution silently discards a value. Limits: key ≤ 4 KiB,
value ≤ 1 MiB, 8192 variables, 4 MiB of key and value bytes combined; exceeding
any of them answers `RESOURCE_EXHAUSTED` with no entries.

Every accepted Request receives exactly one Result. A failure never leaves the
caller waiting for an environment snapshot that cannot arrive.

## Security

**This hands the caller every credential the server was started with.**

That is the whole posture, stated plainly. If the server's environment holds a
`GITHUB_TOKEN`, an `ANTHROPIC_API_KEY`, or any other secret, then any client that
can reach this family reads it. There is no allowlist and no redaction.

Two things bound it, neither of which should be mistaken for a sandbox:

- The ceiling is the one the protocol already has. A client that can call Env
  `GET` can also open a terminal, and a shell prints nearly the same environment.
  This family does not widen who can read what; it removes the need to spawn a
  process to do it. The one difference is `YAS_*`, which `pty/pty_unix.rs`
  strips from a child (`YAS_HUB` excepted) and this family does not: those are
  the server's own knobs — budgets, gates, the socket a caller is already
  talking on — and no credential of the deployment is among them.
- **`YAS_PASSPHRASE` is not one of them.** It belongs to whoever authenticates
  browsers—the YAS edge, or `yas share`—and no server reads it. It
  is not in a server's environment to hand over, and it is kept that way on
  purpose: the CLI's autostart (`transport.rs`) removes it from the child's
  environment rather than trusting that the parent had no reason to hold it,
  because `yas share` reads it one line before autostarting a server.
- **`YAS_ENV=0` makes Environment unavailable at runtime.** The family remains
  selected with `runtime_state = UNAVAILABLE`, and `GET` settles with the common
  `UNAVAILABLE` status and no entries. A client can therefore distinguish an
  operator-disabled family from one the server does not implement.

The asymmetry worth understanding is _who_ is reading. A PTY child prints the
environment because a person typed a command; an extension reads it unattended,
at session start, from code the operator installed once. Installing a persistent
extension is opting in to _running_ that code across restarts, not necessarily
to handing it their credentials. An operator who wants the session-shaped values without the
secrets should set `YAS_ENV=0` and rely on
[processes.md](processes.md)'s session-environment spawn mode, which applies the
session environment to a child **server-side** without ever naming it on the
wire.

## Non-goals

- **No writes.** Nothing sets a server variable. The server's environment is
  fixed at exec, and a family that mutated it would race every reader and every
  in-flight spawn.
- **No watch.** There is no subscription: the value cannot change under a
  running server, so a snapshot is complete by construction.
- **No per-client view.** Every caller sees the same environment. Scoping would
  imply an identity model the protocol does not have.
