# `@doctor`

`@doctor` is the smallest installed extension and the first native QuickJS
one. It checks the path it is running on rather than sending speculative
requests into every server subsystem:

- the v1 handshake and `yas.cli.v1` native channel used by the invocation;
- persistent-extension identity and lifecycle state;
- QuickJS monotonic/realtime clocks and host entropy;
- every capability bit advertised by the server.

An absent optional capability is reported as absent, not broken. The command
returns non-zero only when a check fails.

```bash
./bin/extensions
yas ext run --persist --restart always doctor extensions/dist/doctor.js

yas @doctor
yas @doctor --json
```

`--json` emits one `application/json` result with schema `yas.doctor.v1`.
Putting the root option before the namespace still selects the CLI's NDJSON
transport frames instead: `yas --json @doctor`.

The source is TypeScript for the discriminated report and protocol types. Bun
bundles it with [`../typescript`](../typescript) into the single ECMAScript
module QuickJS evaluates; there are no runtime dependencies.
