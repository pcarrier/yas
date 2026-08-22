# Services

## yas.run

`crates/website` is the only public service for `yas.run`. One Rust process
serves:

- the embedded `js/web` build at `/` and `/s`;
- `install.sh`, `install.ps1`, and `SKILL.md`;
- `/channel/<pubkey>/<producer|consumer>` WebSocket signaling;
- `/ice`, `/message`, and `/health`.

Requests to `/` receive the landing page only when `Accept` includes `text/html`.
All others receive the PowerShell installer for PowerShell user agents, or the
shell installer otherwise. The scripts download stable-name assets directly
from the latest [GitHub release](https://github.com/pcarrier/yas/releases/latest).
`/ext/<asset>` redirects to the same release.

Signaling messages are Ed25519-verified. Redis stores expiring presence and
relays messages between instances. `yas share` and the `/s` browser client use
`wss://yas.run` as their default signaling endpoint.

## Fly.io

Production uses the `yas-887` Fly organization:

- app: `yas-run`;
- region: `cdg` only;
- two shared-CPU Machines with 256 MB each;
- auto-stop disabled;
- one pay-as-you-go managed Redis primary in `cdg`, with no read replicas;
- custom apex domain: `yas.run`.

See [`crates/website/README.md`](crates/website/README.md) for first deployment.
`./bin/deploy-website` deploys from the repository root. Pushes to `main` that
touch the website, browser bundle, installers, or deployment config run
`.github/workflows/deploy-website.yml` using `FLY_API_TOKEN`.

Required secret:

| Secret          | Purpose                       |
| --------------- | ----------------------------- |
| `FLY_API_TOKEN` | Deploy `yas-run` from Actions |

App secrets:

| Secret              | Purpose                             |
| ------------------- | ----------------------------------- |
| `REDIS_URL`         | Managed Redis connection            |
| `CF_TURN_TOKEN_ID`  | Optional Cloudflare TURN credential |
| `CF_TURN_API_TOKEN` | Optional Cloudflare TURN credential |

## CI and releases

CI runs on standard GitHub-hosted Linux, ARM Linux, macOS, and Windows
runners. It parses every Nix file, checks formatting and Clippy, runs Rust,
JavaScript, end-to-end, and coverage tests, builds all release platforms, and
runs a bounded campaign over every YAS wire fuzz target.

Signed tags run the same gates with longer fuzz campaigns, then build Linux and
macOS tarballs plus a Windows zip. The release job uploads both versioned
filenames and stable aliases such as:

```text
yas_linux_x86_64.tar.gz
yas_linux-musl_x86_64.tar.gz
yas_darwin_aarch64.tar.gz
yas_windows_x86_64.zip
```

GitHub release assets are the only binary publication origin; the website
redirects stable download paths there. Archives contain the YAS license and
the Apache-2.0 license for the vendored `yas-alacritty-terminal` engine.
There is no Debian package, APT repository, GPG release-signing path, or
GitHub Pages release site.

The release also publishes workspace crates, JavaScript packages, binary npm
packages, extensions, and the Homebrew update. `./bin/publish-crates` validates
and publishes the fixed-version vendored terminal crate before
`yas-terminal-driver`, waiting for each dependency layer to be indexed.

## Local service installation

`yas generate <prefix>/share` writes man pages and shell completions beside an
installation. Checked-in user units live in [`systemd/`](systemd/); NixOS and
nix-darwin modules are documented in [`nix/README.md`](nix/README.md).

For a user service on Linux:

```sh
systemctl --user enable --now yas.socket
```

System installations can run read-only WebRTC shares with
`yas-share@.service`; each instance reads `/etc/yas/share-<name>.env` for its
passphrase. Persistent authenticated browser access is provided by `yas edge`.
The NixOS and nix-darwin modules expose named `edges` and `shares` options.

For Homebrew on macOS:

```sh
brew services start yas
```
