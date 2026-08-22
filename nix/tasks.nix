{
  pkgs,
  version,
  browserWasm,
  browserWasmNode,
  yas,
  yas-release,
  yas-release-musl ? null,
  yas-release-gnu-gpl ? null,
  yas-release-musl-gpl ? null,
  webAppDist,
  webDist,
  rustToolchain,
  testDbusSessionConfig,
}:
let
  # Helper to set up WASM browser pkg for JS builds.
  setupBrowserPkg = ''
    mkdir -p crates/browser/pkg/snippets
    cp ${browserWasm}/yas_browser.js ${browserWasm}/yas_browser.d.ts crates/browser/pkg/
    cp ${browserWasm}/yas_browser_bg.wasm crates/browser/pkg/
    cp ${browserWasm}/yas_browser_bg.wasm.d.ts crates/browser/pkg/ 2>/dev/null || true
    # An explicit `files` list is required, not cosmetic: crates/browser/pkg
    # is gitignored, and with no `files` and no .npmignore, npm/pnpm packing
    # falls back to .gitignore and drops everything but package.json and
    # main — shipping the package without its .d.ts, which fails the
    # js/web typecheck on a clean checkout.
    echo '{"name":"@yas-run/browser","version":"${version}","files":["yas_browser.js","yas_browser.d.ts","yas_browser_bg.wasm","yas_browser_bg.wasm.d.ts","snippets"],"main":"yas_browser.js","types":"yas_browser.d.ts"}' > crates/browser/pkg/package.json
    if [ -d "${browserWasm}/snippets" ]; then
      for d in ${browserWasm}/snippets/yas-browser-*/; do
        name=$(basename "$d")
        mkdir -p "crates/browser/pkg/snippets/$name"
        cp "$d"/* "crates/browser/pkg/snippets/$name/"
      done
    fi
  '';

  browser-publish = pkgs.writeShellApplication {
    name = "browser-publish";
    runtimeInputs = [ pkgs.nodejs ];
    text = ''
            tmp=$(mktemp -d)
            trap 'rm -rf "$tmp"' EXIT

            cp ${browserWasm}/yas_browser.js "$tmp"/
            cp ${browserWasm}/yas_browser.d.ts "$tmp"/
            cp ${browserWasm}/yas_browser_bg.wasm "$tmp"/
            cp ${browserWasm}/yas_browser_bg.wasm.d.ts "$tmp"/ 2>/dev/null || true
            if [ -d "${browserWasm}/snippets" ]; then
              cp -r ${browserWasm}/snippets "$tmp"/snippets
            fi
            chmod -R u+w "$tmp"

            # Self-initializing Node/Bun build under ./node (see
            # nix/packages.nix `browserWasmNode`).  Exposed via the
            # `@yas-run/browser/node` subpath; the root export stays the
            # `--target web` build so existing browser consumers are unaffected.
            mkdir -p "$tmp/node"
            cp ${browserWasmNode}/yas_browser.js "$tmp/node"/
            cp ${browserWasmNode}/yas_browser.d.ts "$tmp/node"/
            cp ${browserWasmNode}/yas_browser_bg.wasm "$tmp/node"/
            cp ${browserWasmNode}/yas_browser_bg.wasm.d.ts "$tmp/node"/ 2>/dev/null || true
            if [ -d "${browserWasmNode}/snippets" ]; then
              cp -r ${browserWasmNode}/snippets "$tmp/node"/snippets
            fi
            cp ${browserWasmNode}/package.json "$tmp/node"/package.json
            chmod -R u+w "$tmp/node"

            cat > "$tmp/package.json" <<'PKGJSON'
      {
        "name": "@yas-run/browser",
        "version": "${version}",
        "type": "module",
        "description": "Low-latency terminal streaming — browser WASM renderer",
        "main": "yas_browser.js",
        "types": "yas_browser.d.ts",
        "exports": {
          ".": { "types": "./yas_browser.d.ts", "default": "./yas_browser.js" },
          "./node": { "types": "./node/yas_browser.d.ts", "default": "./node/yas_browser.js" },
          "./yas_browser.js": "./yas_browser.js",
          "./yas_browser_bg.wasm": "./yas_browser_bg.wasm",
          "./yas_browser_bg.wasm.d.ts": "./yas_browser_bg.wasm.d.ts",
          "./snippets/*": "./snippets/*",
          "./package.json": "./package.json"
        },
        "files": ["yas_browser_bg.wasm","yas_browser.js","yas_browser.d.ts","yas_browser_bg.wasm.d.ts","snippets","node"],
        "sideEffects": ["./snippets/*"],
        "keywords": ["terminal","tty","wasm","streaming","webgl"],
        "homepage": "https://yas.run",
        "license": "MIT",
        "author": "Indent <oss@indent.com> (https://indent.com)",
        "repository": {"type":"git","url":"git+https://github.com/pcarrier/yas.git","directory":"crates/browser"},
        "bugs": {"url":"https://github.com/pcarrier/yas/issues"}
      }
      PKGJSON
            echo "Package contents:"
            ls -lh "$tmp"
            echo ""
            npm publish "$tmp" "$@"
    '';
  };

  # Publish @yas-run/core, @yas-run/react, @yas-run/solid using the pnpm workspace.
  js-publish = pkgs.writeShellApplication {
    name = "js-publish";
    runtimeInputs = [
      pkgs.nodejs
      pkgs.pnpm
    ];
    text = ''
      pkg_name="$1"
      shift

      tmp=$(mktemp -d)
      trap 'rm -rf "$tmp"' EXIT

      cp -a ${../.}/* "$tmp"/
      chmod -R u+w "$tmp"

      cd "$tmp"
      ${setupBrowserPkg}

      cd js
      pnpm install --frozen-lockfile
      pnpm --filter "$pkg_name" run build

      # pnpm publish resolves workspace:* to real versions
      pnpm --filter "$pkg_name" publish --no-git-checks "$@"
    '';
  };

  publish-npm-packages = pkgs.writeShellApplication {
    name = "yas-publish-npm-packages";
    runtimeInputs = [
      pkgs.nodejs
      pkgs.pnpm
    ];
    text = ''
      echo "=== Publishing @yas-run/browser ==="
      ${browser-publish}/bin/browser-publish "$@"
      echo ""
      echo "=== Publishing @yas-run/core ==="
      ${js-publish}/bin/js-publish @yas-run/core "$@"
      echo ""
      echo "=== Publishing @yas-run/react ==="
      ${js-publish}/bin/js-publish @yas-run/react "$@"
      echo ""
      echo "=== Publishing @yas-run/solid ==="
      ${js-publish}/bin/js-publish @yas-run/solid "$@"
    '';
  };

  publish-crates = pkgs.writeShellApplication {
    name = "yas-publish-crates";
    runtimeInputs = [
      rustToolchain
      pkgs.curl
      pkgs.jq
    ];
    text = ''
      usage() {
        echo "Usage: yas-publish-crates [--plan]"
      }

      plan_only=false
      case $# in
        0) ;;
        1)
          if [ "$1" != "--plan" ]; then
            usage >&2
            exit 2
          fi
          plan_only=true
          ;;
        *)
          usage >&2
          exit 2
          ;;
      esac

      metadata=$(cargo metadata --locked --no-deps --format-version 1)
      vendored_manifest=vendor/yas-alacritty-terminal/Cargo.toml
      vendored_metadata=$(cargo metadata --locked --no-deps --format-version 1 \
        --manifest-path "$vendored_manifest")
      VENDORED_CRATE=$(jq -er '
        if (.packages | length) == 1 and
          .packages[0].name == "yas-alacritty-terminal" and
          (.packages[0].publish == null or
            (.packages[0].publish | index("crates-io")))
        then .packages[0].name
        else error("unexpected vendored terminal crate metadata")
        end
      ' <<<"$vendored_metadata")
      VENDORED_VERSION=$(jq -er '.packages[0].version' <<<"$vendored_metadata")
      jq -e --arg version "$VENDORED_VERSION" '
        any(.packages[] | select(.name == "yas-terminal-driver") |
          .dependencies[];
          .name == "yas-alacritty-terminal" and
          .rename == "alacritty_terminal" and
          .source == null and
          .req == ("=" + $version) and
          (.path | endswith("/vendor/yas-alacritty-terminal")))
      ' <<<"$metadata" >/dev/null

      mapfile -t crates < <(
        jq -r '
          .packages[]
          | select(.publish == null or (.publish | index("crates-io")))
          | .name
        ' <<<"$metadata"
      )
      if [ "''${#crates[@]}" -eq 0 ]; then
        echo "FATAL: workspace has no crates publishable to crates.io" >&2
        exit 1
      fi

      declare -A workspace_crates=()
      while IFS= read -r crate; do
        workspace_crates["$crate"]=1
      done < <(jq -r '.packages[].name' <<<"$metadata")

      declare -A publishable_crates=()
      for crate in "''${crates[@]}"; do
        publishable_crates["$crate"]=1
      done

      dependencies() {
        jq -r --arg crate "$1" '
          .packages[]
          | select(.name == $crate)
          | .dependencies[]
          | select(.kind != "dev" and .path != null)
          | .name
        ' <<<"$metadata"
      }

      for crate in "''${crates[@]}"; do
        while IFS= read -r dependency; do
          if [ -n "''${workspace_crates[$dependency]:-}" ] \
            && [ -z "''${publishable_crates[$dependency]:-}" ]; then
            echo "FATAL: publishable crate $crate depends on non-publishable workspace crate $dependency" >&2
            exit 1
          fi
        done < <(dependencies "$crate")
      done

      declare -A planned=()
      layers=()
      planned_count=0
      while [ "$planned_count" -lt "''${#crates[@]}" ]; do
        layer=()
        for crate in "''${crates[@]}"; do
          [ -n "''${planned[$crate]:-}" ] && continue

          ready=true
          while IFS= read -r dependency; do
            if [ -n "''${publishable_crates[$dependency]:-}" ] \
              && [ -z "''${planned[$dependency]:-}" ]; then
              ready=false
              break
            fi
          done < <(dependencies "$crate")

          if $ready; then
            layer+=("$crate")
          fi
        done

        if [ "''${#layer[@]}" -eq 0 ]; then
          echo "FATAL: workspace crate dependency graph contains a cycle" >&2
          exit 1
        fi

        layers+=("''${layer[*]}")
        for crate in "''${layer[@]}"; do
          planned["$crate"]=1
          planned_count=$((planned_count + 1))
        done
      done

      echo "layer 1: $VENDORED_CRATE"
      layer_number=1
      for layer in "''${layers[@]}"; do
        layer_number=$((layer_number + 1))
        echo "layer $layer_number: $layer"
      done

      $plan_only && exit 0

      if [ -z "''${CARGO_REGISTRY_TOKEN:-}" ] \
        && [ -n "''${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}" ]; then
        echo "=== Exchanging OIDC token for crates.io publish token ==="
        oidc_response=$(curl -sS -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
          "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=crates.io")
        oidc=$(echo "$oidc_response" | jq -r '.value // empty')
        if [ -z "''${oidc:-}" ]; then
          echo "FATAL: failed to get OIDC token from GitHub"
          echo "Response: $oidc_response"
          exit 1
        fi

        token_response=$(curl -sS -X POST https://crates.io/api/v1/trusted_publishing/tokens \
          -H "Content-Type: application/json" \
          -d "{\"jwt\": \"$oidc\"}")
        token=$(echo "$token_response" | jq -r '.token // empty')
        if [ -z "''${token:-}" ]; then
          echo "FATAL: failed to exchange OIDC token for crates.io publish token"
          echo "Response: $token_response"
          exit 1
        fi
        export CARGO_REGISTRY_TOKEN="$token"
      fi

      [ -n "''${CARGO_REGISTRY_TOKEN:-}" ] || { echo "FATAL: no CARGO_REGISTRY_TOKEN and not in GitHub Actions"; exit 1; }

      VERSION=$(jq -r '
        [
          .packages[]
          | select(.publish == null or (.publish | index("crates-io")))
          | .version
        ]
        | unique
        | if length == 1 then .[0] else error("publishable workspace versions differ") end
      ' <<<"$metadata")

      is_published() {
        local crate=$1
        local version=$2
        local code
        code=$(curl -s -o /dev/null -w '%{http_code}' \
          -A 'yas-release/1 (https://github.com/pcarrier/yas)' \
          "https://crates.io/api/v1/crates/$crate/$version")
        [ "$code" = "200" ]
      }

      publish_workspace_crate() {
        if is_published "$1" "$VERSION"; then
          echo "--- $1@$VERSION already published, skipping ---"
          return 0
        fi
        echo "--- publishing $1 ---"
        cargo publish -p "$1" --no-verify
      }

      # Wait until every crate in a layer is indexed on crates.io before
      # proceeding to the next layer.  cargo publish returns before the
      # registry finishes indexing, so without this the next layer would
      # fail with "no matching package" errors.
      wait_for_crate() {
        local crate=$1
        local version=$2
        local attempts=0
        while ! is_published "$crate" "$version"; do
          attempts=$((attempts + 1))
          if [ "$attempts" -ge 60 ]; then
            echo "ERROR: $crate@$version not indexed after 5 minutes, giving up"
            exit 1
          fi
          echo "--- waiting for $crate@$version to be indexed (attempt $attempts/60) ---"
          sleep 5
        done
        echo "--- $crate@$version is available ---"
      }

      if is_published "$VENDORED_CRATE" "$VENDORED_VERSION"; then
        echo "--- $VENDORED_CRATE@$VENDORED_VERSION already published, skipping ---"
      else
        echo "--- publishing $VENDORED_CRATE@$VENDORED_VERSION ---"
        cargo publish --manifest-path "$vendored_manifest" --no-verify --locked
      fi
      wait_for_crate "$VENDORED_CRATE" "$VENDORED_VERSION"

      for layer in "''${layers[@]}"; do
        read -r -a layer_crates <<<"$layer"
        for crate in "''${layer_crates[@]}"; do
          publish_workspace_crate "$crate"
        done
        for crate in "''${layer_crates[@]}"; do
          wait_for_crate "$crate" "$VERSION"
        done
      done
    '';
  };

  deploy-website = pkgs.writeShellApplication {
    name = "deploy-website";
    runtimeInputs = [
      pkgs.flyctl
      pkgs.git
    ];
    text = ''
      root=$(git rev-parse --show-toplevel)
      flyctl deploy "$root" \
        --config "$root/crates/website/fly.toml" \
        --dockerfile "$root/crates/website/Dockerfile" \
        "$@"
    '';
  };

  fmt = pkgs.writeShellApplication {
    name = "yas-fmt";
    runtimeInputs = [
      rustToolchain
      pkgs.prettier
    ];
    text = ''
      check=false
      for arg in "$@"; do
        case "$arg" in
          --check) check=true ;;
        esac
      done

      # extensions/ is a cargo workspace of its own -- it only ever builds for
      # wasm -- so every root-workspace command has to be pointed at it too.
      #
      # From that directory, not with --manifest-path: extensions/Cargo.toml is
      # a virtual manifest, and cargo-fmt pointed at one has no "current crate"
      # to format -- it fails with "Failed to find targets" and prints its
      # usage, which reads like a broken invocation rather than an unformatted
      # tree. `--all` would answer that, but from here it also drags in path
      # dependencies outside the workspace (vendor/), so it formats files this
      # command has no business touching.
      if [ "$check" = true ]; then
        echo "=== cargo fmt --check ==="
        cargo fmt -- --check
        (cd fuzz && cargo fmt -- --check)
        (cd extensions && cargo fmt -- --check)
        echo ""
        echo "=== prettier --check ==="
        prettier --check .
      else
        echo "=== cargo fmt ==="
        cargo fmt
        (cd fuzz && cargo fmt)
        (cd extensions && cargo fmt)
        echo ""
        echo "=== prettier --write ==="
        prettier --write .
      fi
    '';
  };

  clippy = pkgs.writeShellApplication {
    name = "yas-clippy";
    runtimeInputs = [
      rustToolchain
      pkgs.pkg-config
      pkgs.libopus
    ]
    # x264-sys builds in the Linux-only feature-combo passes below: bindgen
    # dlopens nix's libclang, which requires the build scripts themselves to
    # be linked with nix's cc/glibc — a CI runner's system cc links them
    # against an older glibc that cannot load it.
    ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
      pkgs.x264
      pkgs.stdenv.cc
    ];
    text = ''
      export PKG_CONFIG_PATH="${pkgs.libopus.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
      export LIBRARY_PATH="${pkgs.libopus}/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"

      # Clippy only needs these include_bytes! inputs to exist; building the
      # production UI here makes every lint run realize the browser WASM and
      # pnpm closures before Clippy can start. Preserve real development
      # assets, and remove any compile-only placeholders when lint exits.
      echo "=== Setting up UI dist inputs ==="
      mkdir -p js/ui/dist
      placeholder_assets=()
      created_web_dist=false
      cleanup_ui_dist() {
        if (( ''${#placeholder_assets[@]} )); then
          rm -f "''${placeholder_assets[@]}"
        fi
        if [ "$created_web_dist" = true ]; then
          rm -rf js/web/dist
        fi
      }
      trap cleanup_ui_dist EXIT
      for asset in js/ui/dist/index.html.br js/ui/dist/sw.js.br; do
        if [ ! -e "$asset" ]; then
          : > "$asset"
          placeholder_assets+=("$asset")
        fi
      done

      if [ ! -e js/web/dist/index.html ]; then
        mkdir -p js/web/dist
        cp -r ${webDist}/. js/web/dist/
        chmod -R u+w js/web/dist
        created_web_dist=true
      fi

      echo "=== YAS protocol artifacts and compatibility ==="
      cargo xtask protocol --check

      echo "=== Clippy ==="
      cargo clippy --workspace -- -D warnings

      echo "=== Clippy: YAS fuzz harnesses ==="
      cargo clippy --manifest-path fuzz/Cargo.toml --bins -- -D warnings

      # The extensions workspace is excluded from the root one, so it needs its
      # own pass -- and only wasm32 is a meaningful target for it.
      echo "=== Clippy: extensions (wasm32) ==="
      cargo clippy --manifest-path extensions/Cargo.toml \
        --target wasm32-unknown-unknown --release -- -D warnings
    ''
    + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
      # The software H.264 encoders are cargo features (default openh264,
      # x264 as the GPL opt-in, none = AV1-only software fallback) — keep
      # every combination compiling.  x264-sys needs pkg-config + bindgen.
      export PKG_CONFIG_PATH="${pkgs.x264.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
      export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
      export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.lib.getDev pkgs.stdenv.cc.libc}/include"
      cargo clippy -p yas-server --all-targets --all-features -- -D warnings
      cargo clippy -p yas-server --all-targets --no-default-features -- -D warnings
    '';
  };
  protocol-fuzz = pkgs.writeShellApplication {
    name = "yas-protocol-fuzz";
    runtimeInputs = [
      rustToolchain
      pkgs.cargo-fuzz
      # libfuzzer-sys compiles its C++ runtime and Rust's sanitizer build
      # invokes the platform C linker. Keep both tools in the hermetic CI
      # wrapper instead of relying on a developer shell to provide them.
      pkgs.stdenv.cc
    ];
    text = ''
      seconds="''${YAS_FUZZ_SECONDS:-60}"
      if ! [[ "$seconds" =~ ^[1-9][0-9]*$ ]]; then
        echo "YAS_FUZZ_SECONDS must be a positive integer" >&2
        exit 2
      fi

      campaign_root=$(mktemp -d)
      trap 'rm -rf "$campaign_root"' EXIT

      requested="''${YAS_FUZZ_TARGET:-}"
      if [ -n "$requested" ]; then
        case "$requested" in
          frame|families|packed) targets=("$requested") ;;
          *)
            echo "YAS_FUZZ_TARGET must be frame, families, or packed" >&2
            exit 2
            ;;
        esac
      else
        targets=(frame families packed)
      fi

      # cargo-fuzz intentionally requires nightly-only compiler flags. The
      # pinned YAS toolchain builds the same compiler internals; this enables
      # only the harness flags without changing product builds or lockfiles.
      export RUSTC_BOOTSTRAP=1
      for target in "''${targets[@]}"; do
        corpus="$campaign_root/corpus/$target"
        artifacts="$campaign_root/artifacts/$target"
        mkdir -p "$corpus" "$artifacts"
        echo "=== YAS libFuzzer: $target (''${seconds}s) ==="
        cargo fuzz run --fuzz-dir fuzz "$target" "$corpus" -- \
          -max_total_time="$seconds" \
          -max_len=16777216 \
          -timeout=10 \
          -artifact_prefix="$artifacts/"
      done
    '';
  };
  coverage = pkgs.writeShellApplication {
    name = "yas-coverage";
    runtimeInputs = [
      rustToolchain
      pkgs.cargo-llvm-cov
      pkgs.python3
      pkgs.pkg-config
      pkgs.libopus
    ];
    text = ''
      export PKG_CONFIG_PATH="${pkgs.libopus.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
      export LIBRARY_PATH="${pkgs.libopus}/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
      export YAS_WEB_DIST="${webDist}"

      echo "=== Setting up UI dist ==="
      mkdir -p js/ui/dist
      for asset in index.html sw.js; do
        rm -f "js/ui/dist/$asset" "js/ui/dist/$asset.br"
        cp ${webAppDist}/"$asset" ${webAppDist}/"$asset.br" js/ui/dist/
      done

      outdir="''${1:-coverage-report}"

      echo "=== Running tests with coverage ==="
      cargo llvm-cov --no-report --workspace

      echo ""
      echo "=== Coverage summary ==="
      cargo llvm-cov report --json > coverage.json
      python3 ${../bin/format-coverage.py}

      echo ""
      echo "=== Generating HTML report ==="
      cargo llvm-cov report --html --output-dir "$outdir"
      echo "HTML report written to $outdir/html/index.html"
    '';
  };

in
{
  inherit
    browser-publish
    js-publish
    publish-npm-packages
    publish-crates
    deploy-website
    ;
  inherit
    fmt
    clippy
    protocol-fuzz
    coverage
    ;

  # Build every publishable Wasm extension and describe the result.
  #
  # The manifest is the point: an extension is named by its BLAKE3 digest, so a
  # published URL is only pinnable (`ext run <url>#<digest>`) if the digest is
  # published with it.
  extensions = pkgs.writeShellApplication {
    name = "yas-extensions";
    runtimeInputs = [
      rustToolchain
      pkgs.binaryen
      pkgs.b3sum
      pkgs.brotli
      pkgs.bun
      pkgs.jq
      pkgs.typescript
    ];
    text = ''
      cd extensions

      target=wasm32-unknown-unknown
      dist="''${1:-dist}"
      mkdir -p "$dist"

      # The digest is the module's identity, so it must not depend on where
      # the tree happens to sit: rustc bakes absolute paths into panic
      # locations, and a checkout at a different path hashes differently for
      # no reason a reader could ever see.
      cargo_home="''${CARGO_HOME:-$HOME/.cargo}"
      repo_root=$(cd .. && pwd)
      export RUSTFLAGS="--remap-path-prefix=$repo_root=/yas --remap-path-prefix=$cargo_home=/cargo''${RUSTFLAGS:+ $RUSTFLAGS}"

      cargo build --release --target "$target"

      metadata=$(cargo metadata --no-deps --format-version 1)
      version=$(jq -r '.packages[0].version' <<<"$metadata")

      # What each module is for, keyed by the name it is published under.
      # `package.description` is the one place that sentence already lives, and
      # the browser has nothing else to go on: the registry lists names and
      # digests, so without this an installable extension is an opaque word.
      # Keyed by the *bin* target's name because that is what the object is
      # called -- the crate is `yas-ext-systemd`, the module is `systemd.wasm`.
      descriptions=$(jq -c '
        [ .packages[]
          | . as $package
          | .targets[]
          | select(.kind | index("bin"))
          | { key: .name, value: ($package.description // "") }
        ] | from_entries' <<<"$metadata")
      mapfile -t wasm_names < <(jq -r '
        .packages[].targets[]
        | select(.kind | index("bin"))
        | .name' <<<"$metadata")
      entries=()

      # TypeScript extensions are authored against the native QuickJS host,
      # then published as one dependency-free ECMAScript module. QuickJS does
      # not need (and deliberately does not contain) a TypeScript toolchain or
      # module resolver at runtime.
      tsc --project tsconfig.json
      bun test doctor/src typescript
      doctor="$dist/doctor.js"
      bun build doctor/src/main.ts \
        --target=browser --format=esm --minify --outfile="$doctor"
      brotli -f -q 11 -c "$doctor" >"$doctor.br"
      digest=$(b3sum --no-names "$doctor")
      bytes=$(wc -c <"$doctor")
      compressed=$(wc -c <"$doctor.br")
      description=$(jq -r '.description // ""' doctor/package.json)
      entries+=("$(jq -n \
        --arg name "doctor" \
        --arg description "$description" \
        --arg file "doctor.js" \
        --arg digest "$digest" \
        --argjson bytes "$bytes" \
        --argjson compressed "$compressed" \
        '{name: $name, description: $description, file: $file, blake3: $digest, bytes: $bytes, brotli_bytes: $compressed}')")
      printf '%-12s %8s bytes  %8s brotli  %s\n' "doctor" "$bytes" "$compressed" "$digest"

      # Enumerate current manifests, not Cargo's target directory: renamed and
      # removed bins leave build artifacts behind and must not be published.
      for name in "''${wasm_names[@]}"; do
        wasm="target/$target/release/$name.wasm"
        out="$dist/$name.wasm"
        # -Oz over a module that is downloaded once and then cached by digest:
        # the only cost that matters here is bytes on the wire. -all because
        # rustc emits post-MVP features binaryen will not validate without it.
        #
        # Reference types are disabled back off again because -all also lets
        # binaryen *introduce* them, and the server's Wasmi validator rejects a
        # module that uses them: "function references required for index
        # reference types". That failure depends on what the optimizer finds to
        # do, so it appears when an extension grows an indirect call rather than
        # when the flags change — the module runs fine before wasm-opt and is
        # refused after it. The published artifact has to satisfy the loader,
        # not binaryen.
        #
        # Skipped when the input has not moved: cargo is already incremental,
        # and -Oz plus brotli -q 11 is the slow half of this script.
        if [ ! -s "$out" ] || [ "$wasm" -nt "$out" ]; then
          wasm-opt -Oz -all --disable-reference-types --disable-gc \
            --strip-debug --strip-producers "$wasm" -o "$out"
          brotli -f -q 11 -c "$out" >"$out.br"
        fi
        digest=$(b3sum --no-names "$out")
        bytes=$(wc -c <"$out")
        compressed=$(wc -c <"$out.br")
        description=$(jq -r --arg name "$name" '.[$name] // ""' <<<"$descriptions")
        entries+=("$(jq -n \
          --arg name "$name" \
          --arg description "$description" \
          --arg file "$name.wasm" \
          --arg digest "$digest" \
          --argjson bytes "$bytes" \
          --argjson compressed "$compressed" \
          '{name: $name, description: $description, file: $file, blake3: $digest, bytes: $bytes, brotli_bytes: $compressed}')")
        printf '%-12s %8s bytes  %8s brotli  %s\n' "$name" "$bytes" "$compressed" "$digest"
      done

      # The output directory is intentionally incremental, so prune modules
      # that no current Rust bin owns after all current artifacts are ready.
      for out in "$dist"/*.wasm; do
        [ -e "$out" ] || continue
        published=$(basename "$out" .wasm)
        keep=false
        for name in "''${wasm_names[@]}"; do
          if [ "$published" = "$name" ]; then
            keep=true
            break
          fi
        done
        if [ "$keep" = false ]; then
          rm -f "$out" "$out.br"
        fi
      done

      printf '%s\n' "''${entries[@]}" | jq -s \
        --arg version "$version" \
        '{version: $version, extensions: .}' >"$dist/manifest.json"

      echo ""
      echo "wrote $PWD/$dist/manifest.json"
    '';
  };

  build-tarballs = pkgs.writeShellApplication {
    name = "yas-build-tarballs";
    runtimeInputs = [ pkgs.gnutar ];
    text =
      let
        os = if pkgs.stdenv.hostPlatform.isDarwin then "darwin" else "linux";
        arch = if pkgs.stdenv.hostPlatform.isAarch64 then "aarch64" else "x86_64";
        licenseTree = pkgs.runCommand "yas-release-licenses" { } ''
          mkdir -p "$out/licenses"
          cp ${../LICENSE} "$out/LICENSE"
          cp ${../vendor/yas-alacritty-terminal/LICENSE-APACHE} \
            "$out/licenses/yas-alacritty-terminal-APACHE-2.0.txt"
        '';
      in
      ''
        outdir="''${1:-dist/tarballs}"
        mkdir -p "$outdir"
        pack() {
          local output=$1
          local product=$2
          tar --mode='u+w' -czf "$output" \
            -C "$product" bin \
            -C "${licenseTree}" LICENSE licenses
        }
      ''
      + (
        if pkgs.stdenv.hostPlatform.isLinux then
          ''
            # glibc tarball: single binary (all deps statically linked, only glibc dynamic)
            pack "$outdir/yas_${version}_${os}_${arch}.tar.gz" "${yas-release}"
            # musl tarball: single binary (needs system musl libc)
            pack "$outdir/yas_${version}_${os}-musl_${arch}.tar.gz" "${yas-release-musl}"
            # GPL flavors: x264 software H.264 encoder instead of openh264
            # (opt-in via `curl https://yas.run | YAS_GPL=1 sh`)
            pack "$outdir/yas-gpl_${version}_${os}_${arch}.tar.gz" "${yas-release-gnu-gpl}"
            pack "$outdir/yas-gpl_${version}_${os}-musl_${arch}.tar.gz" "${yas-release-musl-gpl}"
          ''
        else
          ''
            # macOS: single binary
            pack "$outdir/yas_${version}_${os}_${arch}.tar.gz" "${yas-release}"
          ''
      )
      + ''
        ls -lh "$outdir"
      '';
  };

  e2e = pkgs.writeShellApplication {
    name = "yas-e2e";
    runtimeInputs = [
      pkgs.nodejs
      # Nix supplies the CI CLI and the matching browser bundle. A local
      # workspace may already have the exact pinned npm CLI installed; the
      # command below uses that one with this same browser bundle so test-file
      # imports do not load a second @playwright/test instance.
      pkgs.playwright-test
    ];
    text = ''
      echo "=== Setting up binaries ==="
      mkdir -p target/debug
      ln -sf "${yas}/bin/yas" target/debug/yas

      echo "=== Running Playwright ==="
      if [ -x e2e/node_modules/.bin/playwright ]; then
        (cd e2e && \
          PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}" \
          ./node_modules/.bin/playwright test "$@")
      else
        (cd e2e && playwright test "$@")
      fi
    '';
  };

  lint = pkgs.writeShellApplication {
    name = "yas-lint";
    runtimeInputs = [
      rustToolchain
      pkgs.git
      pkgs.jq
      pkgs.pkg-config
      pkgs.libopus
    ];
    text = ''
      check=false
      for arg in "$@"; do
        case "$arg" in
          --check) check=true ;;
        esac
      done

      echo "=== Canonical YAS domains and repository ==="
      forbidden_yas_pattern='(^|[^[:alnum:]_-])yas[.]sh([^[:alnum:]_-]|$)|install[.]yas[.]run|indent-com[/]yas|@yas[-]sh|yas[-]sh-browser'
      historical_yas_host="$(printf 'yas.%s' sh)"
      if [ "$(printf '%s\n' "$historical_yas_host" "https://install.$historical_yas_host/path" \
        | grep -Ec "$forbidden_yas_pattern")" -ne 2 ] \
        || printf '%s\n' 'yas.shared.v1' | grep -Eq "$forbidden_yas_pattern"; then
        echo "error: broken canonical YAS domain guard" >&2
        exit 1
      fi
      if git grep -n -I -E \
        "$forbidden_yas_pattern" -- .; then
        echo "error: found a forbidden historical YAS domain or repository" >&2
        exit 1
      fi
      echo ""

      echo "=== Vendored YAS Alacritty dependency ==="
      cargo metadata --locked --format-version 1 | jq -e '
        [.packages[] | select(.name == "yas-alacritty-terminal")] as $engines |
        [.packages[] | select(.name == "yas-terminal-driver") |
          .dependencies[] |
          select(.name == "yas-alacritty-terminal" and
            .rename == "alacritty_terminal")] as $dependencies |
        ($engines | length) == 1 and
        $engines[0].source == null and
        ($engines[0].manifest_path |
          endswith("/vendor/yas-alacritty-terminal/Cargo.toml")) and
        ($dependencies | length) == 1 and
        $dependencies[0].source == null and
        $dependencies[0].req == ("=" + $engines[0].version) and
        ($dependencies[0].path |
          endswith("/vendor/yas-alacritty-terminal"))
      ' >/dev/null
      echo ""

      if [ "$check" = true ]; then
        ${fmt}/bin/yas-fmt --check
      else
        ${fmt}/bin/yas-fmt
      fi
      echo ""
      ${clippy}/bin/yas-clippy
    '';
  };

  tests = pkgs.writeShellApplication {
    name = "yas-tests";
    runtimeInputs = [
      rustToolchain
      pkgs.nodejs
      pkgs.pnpm
      # pnpm's generated POSIX command shims resolve symlinks with sed.
      pkgs.gnused
      pkgs.wasm-pack
      pkgs.wasm-bindgen-cli
      pkgs.python3
      pkgs.bun
      pkgs.typescript
      pkgs.valkey
      pkgs.dbus
      pkgs.pkg-config
      pkgs.libopus
    ]
    # x264-sys builds in the Linux-only feature-combo passes below: bindgen
    # dlopens nix's libclang, which requires the build scripts themselves to
    # be linked with nix's cc/glibc — a CI runner's system cc links them
    # against an older glibc that cannot load it.
    ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
      pkgs.x264
      pkgs.stdenv.cc
    ];
    text = ''
      export PKG_CONFIG_PATH="${pkgs.libopus.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
      export LIBRARY_PATH="${pkgs.libopus}/lib''${LIBRARY_PATH:+:$LIBRARY_PATH}"
      export YAS_TEST_DBUS_SESSION_CONF="${testDbusSessionConfig}"
      export YAS_FONT_DIRS="${pkgs.dejavu_fonts}/share/fonts/truetype"

      # Rust tests exercise the UI routes, so their include_bytes! inputs must
      # be valid Brotli streams with representative content. Building the
      # production UI here would realize the browser WASM and pnpm closures
      # before Cargo can start. Preserve real development assets and remove
      # only the small test fixtures created here.
      echo "=== Setting up UI dist inputs ==="
      mkdir -p js/ui/dist
      placeholder_assets=()
      created_web_dist=false
      valkey_pid=""
      redis_dir=""
      cleanup_ui_dist() {
        if (( ''${#placeholder_assets[@]} )); then
          rm -f "''${placeholder_assets[@]}"
        fi
        if [ "$created_web_dist" = true ]; then
          rm -rf js/web/dist
        fi
        if [ -n "$valkey_pid" ]; then
          kill "$valkey_pid" 2>/dev/null || true
        fi
        if [ -n "$redis_dir" ]; then
          rm -rf "$redis_dir"
        fi
      }
      trap cleanup_ui_dist EXIT
      for asset in js/ui/dist/index.html.br js/ui/dist/sw.js.br; do
        if [ ! -e "$asset" ]; then
          case "$asset" in
            */index.html.br)
              fixture='<!doctype html><html><head><meta charset="utf-8"><title>yas test fixture</title></head><body><main id="root">yas</main></body></html>'
              ;;
            */sw.js.br)
              fixture='/* yas test fixture */ self.addEventListener("fetch", () => {});'
              ;;
          esac
          node -e '
            const fs = require("node:fs");
            const zlib = require("node:zlib");
            fs.writeFileSync(process.argv[1], zlib.brotliCompressSync(Buffer.from(process.argv[2])));
          ' "$asset" "$fixture"
          placeholder_assets+=("$asset")
        fi
      done

      if [ ! -e js/web/dist/index.html ]; then
        mkdir -p js/web/dist
        cp -r ${webDist}/. js/web/dist/
        chmod -R u+w js/web/dist
        created_web_dist=true
      fi

      redis_dir=$(mktemp -d)
      redis_port=$(python3 - <<'PY'
      import socket
      with socket.socket() as sock:
          sock.bind(("127.0.0.1", 0))
          print(sock.getsockname()[1])
      PY
      )
      valkey-server --bind 127.0.0.1 --port "$redis_port" --dir "$redis_dir" \
        --save "" --appendonly no >"$redis_dir/valkey.log" 2>&1 &
      valkey_pid=$!
      for _ in $(seq 1 50); do
        if valkey-cli -p "$redis_port" ping >/dev/null 2>&1; then
          break
        fi
        sleep 0.1
      done
      valkey-cli -p "$redis_port" ping >/dev/null
      export YAS_TEST_REDIS_URL="redis://127.0.0.1:$redis_port"

      echo "=== Rust tests ==="
      cargo test --workspace --all-targets
      echo ""

      # extensions/ is a workspace of its own, so the root run does not reach
      # it. An extension's pure logic — parsing, backoff — is host-testable and
      # is where its bugs live, so run it for the host rather than only linting
      # it for wasm.
      echo "=== Rust tests: extensions ==="
      cargo test --manifest-path extensions/Cargo.toml --workspace --all-targets
      echo ""

      echo "=== TypeScript extension tests ==="
      (cd extensions && tsc --project tsconfig.json && bun test doctor/src typescript)
      echo ""
    ''
    + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
      echo "=== Rust tests: yas-server with both H.264 encoder features ==="
      export PKG_CONFIG_PATH="${pkgs.x264.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
      export LD_LIBRARY_PATH="${pkgs.x264.lib}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
      export LIBCLANG_PATH="${pkgs.llvmPackages.libclang.lib}/lib"
      export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.lib.getDev pkgs.stdenv.cc.libc}/include"
      cargo test -p yas-server --all-features
      echo ""
    ''
    + ''

      # JS tests need the browser module's generated declarations, but not an
      # optimized publish artifact. A local dev build is substantially faster
      # and reuses Cargo's target directory on subsequent runs.
      echo "=== Setting up browser WASM package ==="
      (cd crates/browser && wasm-pack build --target web --dev --out-dir pkg)
      node -e '
        const fs = require("fs");
        const path = "crates/browser/pkg/package.json";
        const pkg = JSON.parse(fs.readFileSync(path, "utf8"));
        pkg.name = "@yas-run/browser";
        fs.writeFileSync(path, JSON.stringify(pkg));
      '

      echo "=== JS typecheck ==="
      (cd js && { pnpm install --frozen-lockfile 2>/dev/null || pnpm install; } && pnpm run typecheck)
      echo ""
      # The core suite includes the bounded, deterministic YAS family and
      # packed-codec property/fuzz corpus (`pnpm test:fuzz` runs it alone).
      echo "=== JS workspace tests (including YAS decoder fuzz corpus) ==="
      # GitHub-hosted Linux runners provide four CPUs. Running four Vitest
      # process pools at once can starve otherwise sub-second module imports
      # past Vitest's five-second timeout, so keep package suites sequential;
      # each suite still uses its own bounded worker pool internally.
      (cd js && pnpm --recursive --workspace-concurrency=1 \
        --filter @yas-run/core \
        --filter @yas-run/react \
        --filter @yas-run/solid \
        --filter @yas-run/ui \
        --filter yas-web \
        run test)

      echo ""
      # Reuse the dependency graph Cargo just compiled instead of making Nix
      # build a separate optimized yas package before this runner can start.
      echo ""
      echo "=== Building fd-channel test server ==="
      cargo build -p yas-cli --bin yas
      cargo_target_dir="''${CARGO_TARGET_DIR:-target}"
      if [[ "$cargo_target_dir" != /* ]]; then
        cargo_target_dir="$PWD/$cargo_target_dir"
      fi
      export YAS_SERVER="$cargo_target_dir/debug/yas"
      echo ""
      echo "=== Python fd-channel test ==="
      python3 examples/fd-channel-python.py
      echo ""
      echo "=== Bun fd-channel test ==="
      bun run examples/fd-channel-bun.ts
    '';
  };
}
