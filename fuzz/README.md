# YAS fuzz targets

These `cargo-fuzz` targets exercise transport framing, the shared extension,
Result, Transfer, and state primitives, at least one complete decoder from
every registered family, and the packed record/grid/frame codecs.

Run them from the repository root, for example:

```sh
direnv exec . ./bin/fuzz
```

`YAS_FUZZ_SECONDS` is the per-target campaign duration. CI runs every target
for 60 seconds; a signed release tag runs three parallel one-hour campaigns
and cannot be published unless all of them finish without a crash.
`YAS_FUZZ_TARGET=frame|families|packed` selects one campaign (the release
matrix uses it); omitting it runs all three. The runner uses temporary corpus
and artifact directories, so an ordinary gate never mutates the checkout. For
local corpus development, `cargo fuzz run --fuzz-dir fuzz <target>` remains
available when `cargo-fuzz` is installed.

Each successful decode is re-encoded or passed to its nested decoder where
that operation is meaningful. The harnesses have no `std` requirement in
`yas-wire`; only libFuzzer's executable wrapper uses the host runtime.
