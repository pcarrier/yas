# YAS canonical schema

The TOML files in this directory are the source of truth for YAS transport,
family, state, status, operation, limit, and packed-codec identifiers. Do not
copy numeric wire identifiers into Rust or TypeScript by hand.

Run the deterministic generator from the repository root with:

```sh
direnv exec . cargo xtask protocol
```

It updates:

- `protocol/yas/schema.json`, the language-neutral registry metadata;
- `protocol/yas/vectors.json`, the shared golden wire vectors; and
- `protocol/yas/generated.rs`, the `no_std` Rust constants, metadata, and
  generated frame-header codec;
- `protocol/yas/wire.md`, the complete human-readable wire tables;
- `protocol/yas/inspection.json`, class/family/kind lookup data for packet
  inspectors and sensitivity-aware diagnostics; and
- `js/core/src/yas/generated.ts`, the TypeScript constants and metadata.

The TypeScript output also contains the matching generated frame-header types,
encoder, and decoder. Family payload types and codecs live in `crates/yas` and
`js/core/src/yas`; both implementations consume generated IDs, policies,
limits, and vectors rather than duplicating the wire registry.

`cargo xtask protocol --check` regenerates everything in memory, rejects a
checked-in diff, and checks the current schema against
`protocol/yas/history/v1.json`. The compatibility check rejects removed or
reused family, kind, status, codec, constant/extension, and limit IDs; removed
family versions; changed required layouts; and changed direction,
sensitivity, compression, datagram, dependency, or hard-limit metadata. New
IDs may only be appended. `--bless-baseline` exists solely to establish a
missing retained protocol-major baseline and refuses to overwrite one; it is
never for an ordinary schema change.

A normal `cargo check -p yas-wire` performs the same artifact and compatibility
checks from the build script. `cargo test -p yas-wire` additionally parses
every registered family and packed codec, decodes every registered full
payload vector, and rejects every proper truncation for values whose wire
shape is not intentionally remainder-consuming. The TypeScript test suite
consumes the same vectors and runs a bounded deterministic arbitrary-byte
corpus through every family decoder and packed codec; run that gate alone with
`(cd js && pnpm --filter @yas-run/core run test:fuzz)`. Host libFuzzer entry
points are under `fuzz/`; their runtime dependency does not change the
`#![no_std]` YAS crate.
