# YAS on Tensorlake

From the checkout root, install the pinned Tensorlake SDK and set your project API key:

```sh
direnv allow
direnv exec . npm ci --prefix bin
export TENSORLAKE_API_KEY=...
```

Node 24 and Nix are required. Import builds `packages.x86_64-linux.yas-release`
from this checkout, including tracked local changes. Add new source files to Git
before building: Nix's Git flake source excludes untracked files. On macOS or
ARM, configure an x86_64 Linux Nix builder; the host's native YAS binary cannot
run in the GPU sandbox.

Build and register the image, then start a sandbox:

```sh
direnv exec . bin/tensorlake-import.ts
direnv exec . bin/tensorlake-start.ts
```

Import registers the private name `yas-ubuntu-26-04` and prints JSON containing
its immutable `cas-v1:…` reference. The upload context contains the built YAS
binary, all locally built extensions and their manifest, the Dockerfile, and
extension installer. Reimport after changing YAS or its extensions; existing sandboxes
continue using their original image.

The image uses Ubuntu 26.04, systemd, PipeWire, htop, Vulkan/EGL userspace libraries, and native
Brave and Firefox DEBs from the [Brave](https://brave.com/linux/) and
[Mozilla](https://support.mozilla.org/en-US/kb/install-firefox-linux) APT
repositories. Mozilla's repository is preferred over Ubuntu's Firefox Snap
launcher. Tensorlake provides the GPU device and driver integration at sandbox
creation.

The image supplies NVIDIA's Vulkan ICD and EGL vendor registration files, plus
the GLVND EGL runtime (`libegl1`). Tensorlake injects the matching NVIDIA driver
libraries at runtime, so the image does not install a separate NVIDIA driver.
Both registrations are required: the Vulkan ICD also loads NVIDIA's EGL vendor.
Without them, YAS can fall back to llvmpipe and spend multiple CPU cores on
software rendering even though `nvidia-smi` sees the GPU.

Check the `yas-server` startup log for `[vulkan-render] initialized: NVIDIA`.
`nvidia-smi` alone does not verify Vulkan rendering. `vulkaninfo --summary` is
also unsuitable as a startup gate here: its display-plane queries can fail on
the sandbox's headless NVIDIA device while YAS renders successfully. Applying
the registration files and EGL packages to an existing sandbox requires a YAS
server restart to replace an already initialized software renderer.

CPU percentages inside the gVisor sandbox need a control measurement.
[gVisor approximates per-process CPU time by sampling task state](https://github.com/google/gvisor/blob/master/pkg/sentry/kernel/task_sched.go),
so frequent timer wakeups can produce misleading `htop`, `/proc`, and
`getrusage` readings. In one Tensorlake sandbox, a Python loop doing nothing
but `time.sleep(0.01)` reported 6–24% CPU. NVIDIA's Vulkan background threads
also wake periodically, even with no surfaces. Compare host cgroup CPU usage
or a host-side profile before treating the displayed percentage as actual
YAS execution time; changing the image cannot change gVisor's accounting.

The image also includes Xwayland and
[xwayland-satellite 0.8.2](https://github.com/Supreeeme/xwayland-satellite/releases/tag/v0.8.2),
built from a checksum-verified upstream source archive in a separate Docker
stage. YAS detects `xwayland-satellite` on `PATH`, starts it with the compositor,
and exports `DISPLAY` for X11 applications. Set `YAS_XWAYLAND=0` to disable it.

Tensorlake owns PID 1 (`tensorlake-init`); an image entrypoint runs as an
ordinary child process and cannot boot systemd. The image keeps a standby
process, and the launcher runs `yas server --share --edge --export-sock
--inject-path` as the managed `yas-server` process and the `yas` user. It waits
for `/run/yas/yas-default.sock` and the edge HTTP listener, then installs every
extension in the bundled manifest (doctor, muster, systemd, and xdg-desktop).
The server uses an always-restart policy and hosts both browser transports.
The systemd extension remains available, but this sandbox
has no systemd manager to inspect.

The edge listens on `0.0.0.0:3264`. Startup exposes that port through Tensorlake's
HTTPS proxy without requiring Tensorlake API authentication; YAS authenticates
connections with the saved passphrase. The returned `edgeUrl` includes that
passphrase in its URL fragment. Existing public ports are preserved. If other
ports still require Tensorlake authentication, startup refuses to disable it
for them: that proxy setting applies to all exposed ports.

Existing persistent extension definitions, including user updates and disabled
extensions, are preserved on subsequent starts. Installation uses the bundled
files and needs no registry download. The launcher creates `/run/yas` with the
correct owner and permissions because the sandbox does not preserve image-time
contents of `/run`.
The sandbox starts with 8 CPUs, 16 GiB RAM, the registered image's root disk size,
and one `RTX-PRO-6000`. Snapshot-backed images cannot shrink; startup leaves the
disk size unset to inherit the image's size. Startup checks the managed YAS server,
GPU visibility, CLI connectivity, and lists the installed extensions.

Optional arguments:

```sh
direnv exec . bin/tensorlake-import.ts --name yas-dev
direnv exec . bin/tensorlake-start.ts --image yas-dev --name my-yas --timeout 3600
direnv exec . bin/tensorlake-start.ts --passphrase "my shared passphrase"
# Retry startup in an existing sandbox, keeping its processes and saved share URL:
direnv exec . bin/tensorlake-start.ts --sandbox SANDBOX_ID
# Pin a particular imported image:
direnv exec . bin/tensorlake-start.ts --image cas-v1:IMAGE_ID
```

`--passphrase` overrides `YAS_SHARE_PASSPHRASE`. If neither is supplied, the
launcher reuses `/etc/yas/share.env` when present, or generates a random
passphrase. Empty or whitespace-only values are rejected before provisioning.
The file stays root-only and is parsed as data; the combined server receives
the value as `YAS_PASSPHRASE` for both transports. Retrying startup preserves a
matching server. Migrating an older sandbox stops its separate `yas-share` and
replaces its server once. Changing the passphrase also replaces the server,
disconnecting its sessions and Wayland applications.

`--sandbox ID` connects to an existing sandbox and runs the same setup without
creating another one. It cannot be combined with `--image`, `--name`, or
`--timeout`; the sandbox keeps its existing configuration. The launcher also
overrides the entrypoint when creating sandboxes, so images built before this
fix work without reimporting.

Start prints the sandbox ID, effective `timeoutSecs`, `edgeUrl`, and `shareUrl` as JSON. Closing the utility releases
SDK handles and leaves the sandbox running. The default is `timeoutSecs: 0`,
which requests the plan maximum: unlimited on Pro and Enterprise. Other plans
still enforce their maximum idle timeout. `--timeout SECONDS` selects a shorter
idle limit. Omitting the API field would use Tensorlake's 10-minute default, so
the launcher sends zero explicitly. See
[Tensorlake's timeout documentation](https://docs.tensorlake.ai/sandboxes/lifecycle#timeout).
When a configured idle timeout expires, ephemeral sandboxes terminate and named
sandboxes suspend. A failed startup check reports the sandbox ID, current state,
and any termination details from Tensorlake. The utility does not terminate it on check failure,
but Tensorlake may already have stopped it. In particular, a proxy
`SANDBOX_NOT_FOUND` after creation can mean the sandbox terminated after
reaching `Running`; the reported lifecycle state explains the 404.

Using the Tensorlake SDK's `sandbox.run`, execute YAS commands as follows:

```ts
await sandbox.run("runuser", {
  args: [
    "-u",
    "yas",
    "--",
    "env",
    "XDG_RUNTIME_DIR=/run/yas",
    "YAS_SOCK=/run/yas/yas-default.sock",
    "yas",
    "terminal",
    "start",
    "brave-browser",
    "--ozone-platform=wayland",
  ],
});
```

Use `sandbox.getOutput("yas-server")` for captured server, edge, and share logs,
or view the managed process in Tensorlake's console.
The launcher enables `YAS_WEBRTC_VERBOSE=1` for ICE and data-channel diagnostics.
The launcher reports extension installation failures directly.

Explicitly terminate a sandbox when finished, using
`Sandbox.connect({ sandboxId })` then `sandbox.terminate()`.

The published TypeScript image wrapper in Tensorlake 0.5.132 does not forward
the CAS flag. `bin/tensorlake/common.ts` calls the native binding shipped with
that SDK using `cas: true` and requires a CAS image ID in the result. This uses
the [SDK's CAS build implementation](https://github.com/tensorlakeai/tensorlake/blob/main/crates/cloud-sdk/src/image_service_builds.rs).
The adapter is intentionally version-pinned; its local HTTP test checks that
the installed SDK targets the CAS Image Service rather than the legacy image
builder. General image workflow: [Tensorlake image documentation](https://docs.tensorlake.ai/sandboxes/images).

```sh
direnv exec . npm --prefix bin run typecheck
direnv exec . npm --prefix bin test
```
