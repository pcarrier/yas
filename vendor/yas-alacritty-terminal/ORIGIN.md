# Source provenance

This directory is the Apache-2.0 Alacritty terminal-library fork used by Yas.
It preserves the published source archive identified by crates.io checksum
`d27f91ac05f3c641d43ba60179a7307b3a73e5fe6862a3fdb42b92e762405a1a` and
upstream VCS commit `ee991a565c6c6dfdc5e75ba6de3c76fddb206f4b`.

The package name, prerelease suffix, description, repository metadata, and
local readme path were changed for the standalone Yas package. The Rust
sources were then normalized once with the repository's stable rustfmt 1.9.0
so the required workspace-wide format check remains deterministic.
`LICENSE-APACHE`, source behavior, tests, changelog, authors, and the upstream
source provenance recorded above are retained. Cargo's registry-unpack marker,
generated VCS file, and original manifest stay in this vendor directory for
auditability but are excluded from the republished crate payload because Cargo
reserves those filenames.
