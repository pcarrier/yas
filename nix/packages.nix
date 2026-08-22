{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    let
      common = import ./common.nix { inherit inputs system; };
      inherit (common)
        pkgs
        pkgsStaticLLVM
        version
        minGlibcVersion
        rustTargetGnu
        cargoLockConfig
        rustToolchain
        rustPlatform
        craneLib
        craneLibStatic
        src
        commonArgs
        commonArgsGnu
        commonArgsStatic
        cargoArtifacts
        cargoArtifactsGnu
        cargoArtifactsStatic
        ;
      serverVaapiEnabled = pkgs.stdenv.hostPlatform.isLinux;
      bindgenClangArgs = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux "-isystem ${pkgs.lib.getDev pkgs.stdenv.cc.libc}/include";
      # Runtime library search path for yas server's dlopen GPU backends.
      #   pkgs.libva           → libva.so.2, libva-drm.so.2
      #   pkgs.libgbm          → libgbm.so.1
      #   pkgs.vulkan-loader   → libvulkan.so.1 (Vulkan dispatch)
      #   addDriverRunpath     → /run/opengl-driver  (libcuda, libnvcuvid,
      #                          libnvidia-encode, Mesa VA-API / Vulkan drivers,
      #                          etc.)
      gpuRuntimeLibPath = pkgs.lib.optionalString serverVaapiEnabled (
        pkgs.lib.makeLibraryPath [
          pkgs.libva
          pkgs.libgbm
          pkgs.vulkan-loader
          pkgs.addDriverRunpath.driverLink
        ]
      );

      # Runtime library search path for yas server's in-process
      # libpipewire-0.3 capture stream.  Loaded via dlopen (see
      # crates/server/src/audio_pw.rs) so it is not in the binary's
      # DT_NEEDED, which means Nix won't patchelf it for us — we have
      # to add its location explicitly.
      audioRuntimeLibPath = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
        pkgs.lib.makeLibraryPath [ pkgs.pipewire ]
      );

      # Combined LD_LIBRARY_PATH for dlopen'd runtime libraries.  Empty
      # on non-Linux where none of the above apply.
      serverRuntimeLibPath = pkgs.lib.concatStringsSep ":" (
        pkgs.lib.optional (gpuRuntimeLibPath != "") gpuRuntimeLibPath
        ++ pkgs.lib.optional (audioRuntimeLibPath != "") audioRuntimeLibPath
      );

      # ------------------------------------------------------------------
      # Crane build
      # ------------------------------------------------------------------

      mkYas =
        pname: featureArgs:
        craneLib.buildPackage (
          commonArgs
          // {
            inherit pname cargoArtifacts;
            nativeBuildInputs =
              commonArgs.nativeBuildInputs ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.makeWrapper;
            cargoExtraArgs = "-p yas-cli ${featureArgs}";
            doCheck = false;
            preBuild = copyWebAppDist;
            postInstall = ''
              $out/bin/yas generate $out/share
            '';
            postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              wrapProgram $out/bin/yas \
                --prefix LD_LIBRARY_PATH : "${serverRuntimeLibPath}"
            '';
            meta.mainProgram = "yas";
          }
        );
      yas = mkYas "yas" "";
      # GPL flavor (x264 instead of openh264; see the release-binaries
      # comment below).
      yas-gpl = mkYas "yas-gpl" gplFeatureArgs;

      # ------------------------------------------------------------------
      # WASM (still uses wasm-pack, not crane)
      # ------------------------------------------------------------------

      # wasm-bindgen requires the CLI version to match the crate version
      # exactly (shared schema). Cargo.lock pins wasm-bindgen 0.2.121, but
      # nixpkgs can lag behind — build the matching CLI here.
      wasmBindgenCli = pkgs.buildWasmBindgenCli rec {
        src = pkgs.fetchCrate {
          pname = "wasm-bindgen-cli";
          version = "0.2.121";
          registryDl = "https://static.crates.io/crates";
          hash = "sha256-ZOMgFNOcGkO66Jz/Z83eoIu+DIzo3Z/vq6Z5g6BDY/w=";
        };
        cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
          inherit src;
          inherit (src) pname version;
          hash = "sha256-DPdCDPTAPBrbqLUqnCwQu1dePs9lGg85JCJOCIr9qjU=";
        };
      };

      browserCargoDeps =
        let
          rawVendorDir = pkgs.rustPlatform.importCargoLock (
            cargoLockConfig
            // {
              extraRegistries = {
                # crates.io's API download endpoint rejects generic fetchers
                # without a Cargo-style User-Agent. Use the static download host
                # for the fetch phase, then remove the extra Cargo source block
                # below because it aliases Cargo's built-in crates-io source.
                "https://github.com/rust-lang/crates.io-index" = "https://static.crates.io/crates";
              };
            }
          );
        in
        pkgs.runCommand "cargo-vendor-dir" { } ''
          mkdir -p "$out"
          cp -R ${rawVendorDir}/. "$out/"

          config="$out/.cargo/config.toml"
          chmod u+w "$out/.cargo" "$config"
          awk '
            /^\[source\."https:\/\/github\.com\/rust-lang\/crates\.io-index"\]$/ { skip = 2; next }
            skip > 0 { skip -= 1; next }
            { print }
          ' "$config" > "$config.tmp"
          mv "$config.tmp" "$config"
        '';

      browserWasm = rustPlatform.buildRustPackage {
        pname = "yas-browser";
        inherit version;
        src = ../.;
        cargoBuildFlags = [
          "-p"
          "yas-browser"
        ];
        cargoDeps = browserCargoDeps;
        nativeBuildInputs = [
          pkgs.wasm-pack
          wasmBindgenCli
          pkgs.binaryen
        ];
        buildPhase = ''
          cd crates/browser
          HOME=$TMPDIR wasm-pack build --target web --release --out-dir $out
        '';
        dontInstall = true;
        doCheck = false;
      };

      # Self-initializing Node/Bun build of the same crate (wasm-bindgen
      # \`--target nodejs\`).  Unlike the \`--target web\` build, this one reads its
      # \`.wasm\` from disk and instantiates synchronously on import, so it works
      # off-browser with no \`fetch\`/\`init()\` dance.  Published under the
      # \`@yas-run/browser/node\` subpath; see nix/tasks.nix \`browser-publish\`.
      browserWasmNode = rustPlatform.buildRustPackage {
        pname = "yas-browser-node";
        inherit version;
        src = ../.;
        cargoBuildFlags = [
          "-p"
          "yas-browser"
        ];
        cargoDeps = browserCargoDeps;
        nativeBuildInputs = [
          pkgs.wasm-pack
          wasmBindgenCli
          pkgs.binaryen
        ];
        buildPhase = ''
          cd crates/browser
          HOME=$TMPDIR wasm-pack build --target nodejs --release --out-dir $out
          # wasm-bindgen's nodejs target emits CommonJS glue (require/__dirname)
          # but copies the inline JS snippets verbatim as ES modules, so the
          # generated require() of each snippet throws under Node.  Rewrite the
          # snippets to CommonJS.  (They are canvas-only helpers, never invoked
          # in a headless terminal, but must still load.)
          for f in $out/snippets/yas-browser-*/*.js; do
            names=$(grep -oE 'export function [A-Za-z0-9_]+' "$f" \
              | sed 's/export function //' | paste -sd, -)
            sed -i 's/^export function /function /' "$f"
            if [ -n "$names" ]; then
              printf '\nmodule.exports = { %s };\n' "$names" >> "$f"
            fi
          done
          # Mark this subtree as CommonJS so it loads correctly when nested
          # under the package root's "type":"module".
          printf '{"type":"commonjs","main":"yas_browser.js","types":"yas_browser.d.ts"}\n' \
            > $out/package.json
        '';
        dontInstall = true;
        doCheck = false;
      };

      # ------------------------------------------------------------------
      # Release binaries
      #
      # Linux ships two variants:
      #   glibc (yas-gnu)  — dynamically linked, dlopen works for GPU
      #   musl  (yas-musl) — all deps statically linked except musl libc
      # Each Linux variant also has a "-gpl" flavor that swaps the openh264
      # software H.264 encoder for x264 (GPL-2.0-or-later) — a deliberate
      # opt-in via `curl https://yas.run | YAS_GPL=1 sh`; see
      # `yas --license`.  macOS ships a single binary with nix-store
      # dylibs rewritten to system paths (no software H.264 encoder — the
      # compositor is Linux-only).
      # ------------------------------------------------------------------

      # Cargo feature flags for the GPL flavor: x264 instead of openh264.
      gplFeatureArgs = "--no-default-features --features x264";

      # Linux glibc binary — all deps statically linked, only glibc is
      # dynamic (so dlopen works for GPU).  Built with cargo-zigbuild
      # targeting glibc ${minGlibcVersion} for broad distro compat.
      mkYasGnu =
        pname: featureArgs:
        craneLib.buildPackage (
          commonArgsGnu
          // {
            inherit pname;
            cargoArtifacts = cargoArtifactsGnu;
            cargoExtraArgs = "-p yas-cli ${featureArgs}";
            doCheck = false;
            preBuild = copyWebAppDist;
            buildPhaseCargoCommand = "HOME=$TMPDIR cargo zigbuild --release --target ${rustTargetGnu}.${minGlibcVersion} -p yas-cli ${featureArgs}";
            doNotPostBuildInstallCargoBinaries = true;
            installPhaseCommand = ''
              mkdir -p $out/bin
              cp target/${rustTargetGnu}/release/yas $out/bin/
            '';
          }
        );
      yas-gnu = mkYasGnu "yas-gnu" "";
      yas-gnu-gpl = mkYasGnu "yas-gnu-gpl" gplFeatureArgs;

      # Linux musl dynamic binary — all deps statically linked except
      # musl libc.  For Alpine and other musl-based systems.
      mkYasMusl =
        pname: featureArgs:
        craneLibStatic.buildPackage (
          commonArgsStatic
          // {
            inherit pname;
            cargoArtifacts = cargoArtifactsStatic;
            cargoExtraArgs = "-p yas-cli ${featureArgs}";
            doCheck = false;
            preBuild = copyWebAppDist;
            dontPatchELF = true;
            postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              for bin in $out/bin/*; do
                interp=$(readelf -l "$bin" 2>/dev/null \
                  | grep -oP 'Requesting program interpreter: \K[^\]]+' || true)
                case "$(basename "$interp")" in
                  ld-musl-*) ;;
                  *) echo "FATAL: expected musl interpreter, got: $interp"; exit 1 ;;
                esac
                needed=$(readelf -d "$bin" 2>/dev/null \
                  | grep -oP '\(NEEDED\)\s+Shared library: \[\K[^\]]+' || true)
                for lib in $needed; do
                  case "$lib" in
                    libc.so) ;;
                    *) echo "FATAL: unexpected NEEDED library: $lib"; exit 1 ;;
                  esac
                done
              done
            '';
          }
        );
      yas-musl = mkYasMusl "yas-musl" "";
      yas-musl-gpl = mkYasMusl "yas-musl-gpl" gplFeatureArgs;

      # Assembled glibc release: single binary with system interpreter.
      # All deps are statically linked; only glibc is dynamic.
      mkReleaseGnu =
        name: drv:
        let
          interpreter =
            if pkgs.stdenv.hostPlatform.isAarch64 then
              "/lib/ld-linux-aarch64.so.1"
            else
              "/lib64/ld-linux-x86-64.so.2";
        in
        pkgs.runCommand "${name}-${version}"
          {
            nativeBuildInputs = [ pkgs.patchelf ];
          }
          ''
            mkdir -p $out/bin
            cp ${drv}/bin/yas $out/bin/yas
            chmod +w $out/bin/yas
            patchelf --set-interpreter ${interpreter} $out/bin/yas
            patchelf --remove-rpath $out/bin/yas
          '';
      yas-release-gnu = mkReleaseGnu "yas-release-gnu" yas-gnu;
      yas-release-gnu-gpl = mkReleaseGnu "yas-release-gnu-gpl" yas-gnu-gpl;

      # Assembled musl release: single binary, interpreter set to
      # system musl path.
      mkReleaseMusl =
        name: drv:
        let
          arch = if pkgs.stdenv.hostPlatform.isAarch64 then "aarch64" else "x86_64";
        in
        pkgs.runCommand "${name}-${version}"
          {
            nativeBuildInputs = [ pkgs.patchelf ];
          }
          ''
            mkdir -p $out/bin
            cp ${drv}/bin/yas $out/bin/yas
            chmod +w $out/bin/yas
            patchelf --set-interpreter /lib/ld-musl-${arch}.so.1 $out/bin/yas
          '';
      yas-release-musl = mkReleaseMusl "yas-release-musl" yas-musl;
      yas-release-musl-gpl = mkReleaseMusl "yas-release-musl-gpl" yas-musl-gpl;

      # Default release package per platform.
      yas-release =
        if pkgs.stdenv.hostPlatform.isLinux then
          yas-release-gnu
        else
          # macOS: rewrite nix-store dylibs to system paths.
          craneLibStatic.buildPackage (
            commonArgsStatic
            // {
              pname = "yas-release";
              cargoArtifacts = cargoArtifactsStatic;
              cargoExtraArgs = "-p yas-cli";
              doCheck = false;
              preBuild = copyWebAppDist;
              postFixup = ''
                for bin in $out/bin/*; do
                  for lib in $(otool -L "$bin" | tail -n +2 | awk '/\/nix\/store\//{print $1}'); do
                    base=$(basename "$lib")
                    case "$base" in
                      libiconv.*|libiconv-*) sys="/usr/lib/libiconv.2.dylib" ;;
                      libz.*|libz-*) sys="/usr/lib/libz.1.dylib" ;;
                      libc++.*) sys="/usr/lib/libc++.1.dylib" ;;
                      libc++abi.*) sys="/usr/lib/libc++abi.dylib" ;;
                      libresolv.*) sys="/usr/lib/libresolv.9.dylib" ;;
                      libSystem.*) sys="/usr/lib/libSystem.B.dylib" ;;
                      *) echo "FATAL: unknown nix-store dylib: $lib"; exit 1 ;;
                    esac
                    echo "rewriting $lib -> $sys"
                    install_name_tool -change "$lib" "$sys" "$bin"
                  done
                done
              '';
            }
          );

      # ------------------------------------------------------------------
      # JS / Web assets
      # ------------------------------------------------------------------

      # A `path:.` flake includes ignored working-tree files.  In particular,
      # a developer's node_modules would otherwise reach pnpmConfigHook,
      # which correctly tries to replace it but cannot confirm the removal in
      # a non-interactive Nix build.  Keep untracked source files (useful for
      # testing dirty trees) while pruning only generated dependency trees.
      webSource = pkgs.lib.cleanSourceWith {
        src = ../.;
        filter = path: type: type != "directory" || baseNameOf path != "node_modules";
      };

      setupBrowserPkg = ''
        mkdir -p crates/browser/pkg/snippets
        cp ${browserWasm}/yas_browser.js crates/browser/pkg/
        cp ${browserWasm}/yas_browser_bg.wasm crates/browser/pkg/
        cp ${browserWasm}/yas_browser.d.ts crates/browser/pkg/
        cp ${browserWasm}/yas_browser_bg.wasm.d.ts crates/browser/pkg/
        # An explicit `files` list is required, not cosmetic: crates/browser/pkg
        # is gitignored, and with no `files` and no .npmignore, npm/pnpm packing
        # falls back to .gitignore and drops everything but package.json and
        # main — which silently ships the package without its .d.ts and .wasm,
        # failing the website typecheck on a clean checkout.
        echo '{"name":"@yas-run/browser","version":"${version}","files":["yas_browser.js","yas_browser.d.ts","yas_browser_bg.wasm","yas_browser_bg.wasm.d.ts","snippets"],"main":"yas_browser.js","types":"yas_browser.d.ts"}' > crates/browser/pkg/package.json
        for d in ${browserWasm}/snippets/yas-browser-*/; do
          name=$(basename "$d")
          mkdir -p "crates/browser/pkg/snippets/$name"
          cp "$d"/* "crates/browser/pkg/snippets/$name/"
        done
      '';

      # fetchPnpmDeps hashes the pnpm store, including the packed local
      # file:../../crates/browser/pkg dependency.  Do not feed it the real
      # browserWasm output: its store path differs by platform/toolchain and
      # would make pnpmDeps.hash churn even when js/pnpm-lock.yaml is stable.
      setupBrowserPkgForDeps = ''
        mkdir -p crates/browser/pkg/snippets
        cat > crates/browser/pkg/package.json <<'EOF'
        {"name":"@yas-run/browser","version":"${version}","files":["yas_browser.js","yas_browser.d.ts","yas_browser_bg.wasm","yas_browser_bg.wasm.d.ts","snippets"],"main":"yas_browser.js","types":"yas_browser.d.ts"}
        EOF
        : > crates/browser/pkg/yas_browser.js
        : > crates/browser/pkg/yas_browser_bg.wasm
        : > crates/browser/pkg/yas_browser.d.ts
        : > crates/browser/pkg/yas_browser_bg.wasm.d.ts
      '';

      # pnpm 11's worker pool reliably crashes node's event loop inside the
      # macOS Nix sandbox right after "added N, done":
      #   Assertion failed: (errno == EINTR), function uv__io_poll, kqueue.c
      # (SIGABRT, builder exit 134).  Raising `ulimit -n` and
      # `trustLockfile: true` were both tried on v0.34.0 and did not help.
      # Pin pnpm 10 for everything that runs inside the sandbox until the
      # libuv/pnpm 11 kqueue issue is fixed upstream (nixpkgs is doing the
      # same for affected packages, e.g. NixOS/nixpkgs#529330).  The pnpm
      # version must match between fetchPnpmDeps and the consuming builds:
      # the packed store layout (v10 vs v11) differs.
      pnpmSandboxed = pkgs.pnpm_10;

      pnpmDeps = pkgs.fetchPnpmDeps {
        pname = "yas-js";
        inherit version;
        src = webSource;
        pnpm = pnpmSandboxed;
        fetcherVersion = 3;
        postPatch = setupBrowserPkgForDeps + ''
          cd js
        '';
        hash = "sha256-SK09VElUMuRYDUGvYnkmhx0uuAb4tpyvYaKUAnSTLwk=";
      };

      webAppDist = pkgs.stdenv.mkDerivation {
        pname = "yas-ui";
        inherit version;
        src = webSource;
        inherit pnpmDeps;
        nativeBuildInputs = [
          pkgs.nodejs
          pnpmSandboxed
          pkgs.pnpmConfigHook
        ];
        pnpmRoot = "js";
        postPatch = setupBrowserPkg;
        buildPhase = ''
          cd js
          pnpm --filter @yas-run/core run build
          pnpm --filter @yas-run/solid run build
          pnpm --filter @yas-run/ui run build
        '';
        installPhase = ''
          mkdir -p $out
          # The service worker ships separately from the single-file app and
          # the edge embeds it by name.
          cp ui/dist/index.html ui/dist/index.html.br ui/dist/sw.js ui/dist/sw.js.br \
            $out/
        '';
        doCheck = false;
      };

      webDist = pkgs.stdenv.mkDerivation {
        pname = "yas-web";
        inherit version;
        src = webSource;
        inherit pnpmDeps;
        nativeBuildInputs = [
          pkgs.nodejs
          pnpmSandboxed
          pkgs.pnpmConfigHook
        ];
        pnpmRoot = "js";
        postPatch = setupBrowserPkg;
        buildPhase = ''
          cd js
          pnpm --filter yas-web run build
        '';
        installPhase = ''
          mkdir -p $out
          cp -r web/dist/* $out/
        '';
        doCheck = false;
      };

      copyWebAppDist = ''
        mkdir -p js/ui/dist
        cp ${webAppDist}/index.html ${webAppDist}/index.html.br \
          ${webAppDist}/sw.js ${webAppDist}/sw.js.br js/ui/dist/
      '';

      website = craneLib.buildPackage (
        commonArgs
        // {
          pname = "yas-website";
          src = webSource;
          inherit cargoArtifacts;
          cargoExtraArgs = "-p yas-website";
          doCheck = false;
          preBuild = ''
            mkdir -p js/web/dist
            cp -r ${webDist}/. js/web/dist/
          '';
          meta.mainProgram = "yas-website";
        }
      );

      # Man pages and shell completions are generated by `yas generate`
      # during postInstall.

      # ------------------------------------------------------------------
      # Tasks
      # ------------------------------------------------------------------

      tasks = import ./tasks.nix {
        inherit
          pkgs
          version
          browserWasm
          browserWasmNode
          yas
          yas-release
          webAppDist
          webDist
          rustToolchain
          ;
        yas-release-musl = if pkgs.stdenv.hostPlatform.isLinux then yas-release-musl else null;
        yas-release-gnu-gpl = if pkgs.stdenv.hostPlatform.isLinux then yas-release-gnu-gpl else null;
        yas-release-musl-gpl = if pkgs.stdenv.hostPlatform.isLinux then yas-release-musl-gpl else null;
      };

    in
    {
      packages = {
        inherit
          yas
          yas-release
          website
          ;
        inherit pnpmDeps;
        default = yas;
      }
      // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
        inherit
          yas-gpl
          yas-release-musl
          yas-release-gnu-gpl
          yas-release-musl-gpl
          ;
      }
      // tasks;

      devShells.default = pkgs.mkShell {
        buildInputs = [
          rustToolchain
          pkgs.rust-analyzer
          # An extension's identity is its BLAKE3 digest, so checking one by
          # hand against a manifest needs a tool that speaks it.
          pkgs.b3sum
          pkgs.binaryen
          pkgs.bun
          # gdbus, so the systemd extension can watch units by signal rather
          # than by polling when it is run from a development shell.
          pkgs.glib.bin
          pkgs.cargo-flamegraph
          pkgs.cargo-llvm-cov
          pkgs.cargo-edit
          pkgs.cmake
          pkgs.cargo-watch
          pkgs.curl
          pkgs.flyctl
          pkgs.libopus
          pkgs.nodejs
          pkgs.pkg-config
          pkgs.pkgsStatic.stdenv.cc
          pkgs.pnpm
          pkgs.samply
          pkgs.socat
          pkgs.wasm-bindgen-cli
          pkgs.wasm-pack
          # Language servers, so `yas lsp` (docs/design/lsp.md) is
          # dogfoodable across this polyglot repo. rust-analyzer is
          # above; these cover Nix, shell, TS/JS, TOML, YAML, and
          # Markdown. yas discovers them on PATH — none is required.
          pkgs.nixd
          pkgs.bash-language-server
          pkgs.taplo
          pkgs.yaml-language-server
          pkgs.marksman
          pkgs.typescript-language-server
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          pkgs.dbus
          pkgs.pipewire
          pkgs.wireplumber
          # `rtkitctl`, to check that the audio graph can actually get the
          # realtime priority it asks for. A server run from this shell has no
          # systemd unit and no session, so RTKit is its only route to
          # SCHED_FIFO — and PipeWire carries on silently without it, running
          # the data loop at ordinary priority against the video encoders and
          # cutting gaps into captured audio the moment a core saturates.
          # `rtkitctl --start` brings the daemon up on a host that has it
          # installed but not running.  Linux-only: rtkit has no Darwin
          # build, and listing it unconditionally fails the whole devShell
          # evaluation there.
          pkgs.rtkit
          # The X11 bridge a session starts by name when it is present
          # (crates/server/src/xwayland.rs); without it a dev server is
          # Wayland-only, which is a different thing to be testing.
          pkgs.xwayland-satellite
          pkgs.llvmPackages.libclang
          pkgs.x264
        ];

        shellHook = ''
          if [ -z "''${LANG-}" ]; then
            export LANG="$(defaults read -g AppleLocale 2>/dev/null | sed 's/@.*//' || echo en_US).UTF-8"
          fi
          export BINDGEN_EXTRA_CLANG_ARGS="${bindgenClangArgs}''${NIX_CFLAGS_COMPILE:+ $NIX_CFLAGS_COMPILE}"
          export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
          export PKG_CONFIG_PATH="${pkgs.libopus.dev}/lib/pkgconfig${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ":${pkgs.x264.dev}/lib/pkgconfig"}''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
          # LIBRARY_PATH is propagated by nix-direnv while LD_LIBRARY_PATH
          # is filtered out — .envrc reconstructs LD_LIBRARY_PATH from
          # LIBRARY_PATH, so anything we need at runtime has to land
          # here.  libopus is for the Opus encoder; libx264 for the H.264
          # software surface encoder; libpipewire-0.3 is dlopened by the
          # server's in-process audio capture path (see
          # crates/server/src/audio_pw.rs).
          export LIBRARY_PATH="${pkgs.libopus}/lib${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ":${pkgs.x264.lib}/lib:${pkgs.pipewire}/lib"}''${LIBRARY_PATH:+:$LIBRARY_PATH}"
          # Runtime dlopen: yas server loads VA-API / NVENC GPU libs,
          # libpipewire-0.3, and native camera decoders at runtime. See the
          # serverRuntimeLibPath definition above.  (Direct LD_LIBRARY_PATH
          # export is effective for plain `nix develop`; under direnv,
          # the .envrc reconstruction from LIBRARY_PATH takes over.)
          ${pkgs.lib.optionalString (
            serverRuntimeLibPath != ""
          ) ''export LD_LIBRARY_PATH="${serverRuntimeLibPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"''}
          # The portal frontend yas spawns for its private bus (see
          # crates/server/src/desktop_bus.rs) is found by PATH lookup, but
          # this package ships no bin/ — the binary is under libexec/, so a
          # plain `packages` entry would put nothing on PATH and the frontend
          # would silently never start. Without it a viewer's camera is
          # invisible to Firefox and Chromium (both reach cameras through
          # org.freedesktop.portal.Camera) and getDisplayMedia falls back to
          # in-browser tab capture.
          ${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''export PATH="${pkgs.xdg-desktop-portal}/libexec:$PATH"''}
          export PATH="$PWD/target/profiling:$PWD/bin:$PATH"
        '';
      };
    };
}
