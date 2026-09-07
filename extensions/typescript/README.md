# TypeScript extension support

These are source libraries, not runtime modules. A TypeScript extension imports
them normally and the build publishes one bundled ECMAScript module. QuickJS
has no package resolver and does not transpile TypeScript.

- [`yas.ts`](yas.ts) types the frozen `yas.context` and native host calls,
  and supplies UTF-8 and little-endian codecs without browser globals.
- [`command.ts`](command.ts) registers and serves a small synchronous
  `yas.cli.v1` command provider.

The command helper is intentionally for bounded diagnostic and configuration
responses. QuickJS also exposes synchronous, capability-checked bridges for
larger external extensions: command stdin, `supports`, Channel limits, Env,
Process capture, read-only Git inspection, FS index/read/write, BLAKE3, and a
deadline-bounded raw Net exchange. Process and Net calls continue polling the
active invocation, so `CANCEL` terminates their work rather than waiting for a
child or socket indefinitely. Paths passed to FS read/write remain relative to
the separately supplied native root.

These bridges use the ordinary selected YAS families and their negotiated
limits; they do not grant direct operating-system APIs to QuickJS. A provider
still serves one active invocation at a time. Cooperative multi-process
schedulers should stay in Rust until the JavaScript bridge grows non-blocking
spawn and event-routing handles.

[`@doctor`](../doctor) is the complete example: a typed report, a human
renderer, a structured JSON result, protocol tests, and a default export that
stays small.
