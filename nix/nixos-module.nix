self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.yas;
  inherit (lib)
    mkEnableOption
    mkOption
    types
    mkIf
    ;

  # yas server loads GPU codec libraries via dlopen at runtime:
  #   VA-API:  libva.so.2, libva-drm.so.2  (from pkgs.libva)
  #   NVDEC:   libcuda.so.1, libnvcuvid.so.1         (from the GPU driver)
  #   NVENC:   libcuda.so.1, libnvidia-encode.so.1   (from the GPU driver)
  # On NixOS these live under /nix/store and are not in the default
  # ld.so search path.  addDriverRunpath.driverLink is the NixOS-managed
  # symlink farm (/run/opengl-driver) for the active GPU driver (NVIDIA,
  # Mesa, etc.) and covers NVENC, CUDA, and VA-API backend drivers.
  gpuLibSearchPath = lib.makeLibraryPath (cfg.gpuLibraries ++ [ pkgs.addDriverRunpath.driverLink ]);

  # The server also dlopens libpipewire-0.3.so.0 directly when audio is
  # enabled (replacing the former pw-cat subprocess).  Add pipewire's
  # library dir to the loader path so the dlopen resolves.
  audioLibSearchPath = lib.makeLibraryPath [ pkgs.pipewire ];

  # An edge or a share belongs to a server, and runs inside it: one unit to
  # supervise, and a browser or WebRTC consumer that reaches the terminals
  # without a socket in between. A share hosted this way also does without the
  # yas-proxy daemon, which exists to pool the socket connections it no longer
  # makes. Both are keyed by user, because a server has at most one of each.
  edgeFor = user: cfg.edges.${user} or null;
  shareFor = user: cfg.shares.${user} or null;
  hostedEnvFor =
    user:
    lib.optionals (edgeFor user != null) (
      [
        "YAS_EDGE=1"
        "YAS_ADDR=${(edgeFor user).addr}:${toString (edgeFor user).port}"
      ]
      ++ lib.optional (
        (edgeFor user).trustedProxyIps != [ ]
      ) "YAS_TRUSTED_PROXY_IPS=${lib.concatStringsSep "," (edgeFor user).trustedProxyIps}"
    )
    ++ lib.optionals (shareFor user != null) (
      [ "YAS_SHARE=1" ]
      ++ lib.optional (!(shareFor user).quiet) "YAS_SHARE_QUIET=0"
      ++ lib.optional (shareFor user).verbose "YAS_SHARE_VERBOSE=1"
      ++ lib.optional ((shareFor user).hub != null) "YAS_HUB=${(shareFor user).hub}"
      ++ lib.optional (shareFor user).verboseWebrtc "YAS_WEBRTC_VERBOSE=1"
    );
  # One file listed once, even when the same secret answers for both.
  hostedPassFilesFor =
    user:
    lib.unique (
      lib.optional (edgeFor user != null) (edgeFor user).passFile
      ++ lib.optional (shareFor user != null) (shareFor user).passFile
    );

  # Combined LD_LIBRARY_PATH for the server unit. GPU and audio paths remain
  # conditional; software camera decoders are compiled into yas.
  serverLibSearchPath = lib.concatStringsSep ":" (
    lib.optional (gpuLibSearchPath != "") gpuLibSearchPath
    ++ lib.optional cfg.audio.enable audioLibSearchPath
  );

  # Resolve the user's normal Nix profiles once for both PATH and
  # `XDG_DATA_DIRS`. The latter is where the list of installed applications
  # comes from: the `session` extension uses native Env GET and FS INDEX/READ to
  # scan `$XDG_DATA_DIRS/*/applications` for `.desktop` files
  # (extensions/session/src/main.rs). A unit inherits none of the login
  # environment, so without this the extension falls back to the spec's default
  # of `/usr/local/share:/usr/share` — neither of which exists on NixOS — and the
  # only applications anyone can launch are whatever happens to be under
  # `~/.local/share/applications`. Everything installed through a Nix profile is
  # invisible.
  #
  # `environment.profiles` is the same list `/etc/profile` turns into
  # `XDG_DATA_DIRS` for an interactive shell, so a session sees the applications
  # its user would see on the console, in the same precedence order, including
  # whatever other modules (Flatpak) have added.
  userProfileRoots =
    user:
    let
      home = lib.attrByPath [ user "home" ] "/home/${user}" config.users.users;
      # systemd does no shell expansion in `Environment=`, so a profile written
      # in terms of another variable would land as a literal and resolve to
      # nothing. Drop those rather than ship a root that cannot exist.
      resolvable = lib.filter (profile: !(lib.hasInfix "\${" profile)) config.environment.profiles;
    in
    map (profile: lib.replaceStrings [ "$HOME" "$USER" ] [ home user ] profile) resolvable;

  userDataDirs =
    user: lib.concatMapStringsSep ":" (profile: profile + "/share") (userProfileRoots user);
in
{
  options.services.yas = {
    enable = mkEnableOption "yas terminal multiplexer";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.yas;
      defaultText = "self.packages.\${system}.yas";
      description = "The yas package to use.";
    };

    users = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [
        "alice"
        "bob"
      ];
      description = ''
        Users to enable yas for. Each user gets a continuously running
        yas server instance at /run/yas/<user>/yas-default.sock.
      '';
    };

    shell = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/current-system/sw/bin/bash";
      description = "Shell to spawn for new PTYs. Defaults to the user's login shell.";
    };

    scrollback = mkOption {
      type = types.int;
      default = 10000;
      description = "Scrollback buffer size in rows per PTY.";
    };

    languageServers = mkOption {
      type = types.listOf types.package;
      default = [ ];
      example = lib.literalExpression "[ pkgs.nixd pkgs.rust-analyzer pkgs.gopls ]";
      description = ''
        Language servers to place on the yas server's PATH so
        <literal>yas lsp</literal> (docs/design/lsp.md) can discover and
        spawn them. yas ships none; list the servers you want available
        and their binaries are added to the server process's PATH. yas
        matches them to files by project marker and extension, keeps them
        warm across connections, and never downloads anything. Empty by
        default (the family is advertised but finds no servers). Set
        <option>YAS_LSP=0</option> via the environment to disable the
        family entirely.
      '';
    };

    extensions = {
      persistent = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Permit durable Wasm and JavaScript extensions (<literal>yas ext run
          --persist</literal>) and start the ones that should be running again
          after a restart. This is also what makes an extension's
          <literal>@name</literal> command namespace exist. Setting it false
          passes <option>YAS_ALLOW_EXT_PERSIST=0</option>, which is the
          recovery path for a persistent definition that breaks the server it
          starts in; transient extensions still run without it.

          Definitions live in
          <filename>~/.local/state/yas/instances/default/extensions.redb</filename>
          and module bytes in
          <filename>~/.cache/yas/instances/default/wasm</filename>, so
          clearing the cache blocks every persistent extension until one is
          uploaded again.
        '';
      };

      path = mkOption {
        type = types.listOf types.package;
        default = [ ];
        example = lib.literalExpression "[ pkgs.glib.bin ]";
        description = ''
          Extra packages on the yas server's PATH, for the processes
          extensions spawn. An extension reaches the machine only through
          protocol operations such as starting a child process, and a server
          started from this unit has little more than coreutils and systemd on
          its PATH — so whatever an extension shells out to belongs here.

          <literal>pkgs.glib.bin</literal> supplies <literal>gdbus</literal>,
          which lets the systemd extension react to unit changes as they
          happen instead of polling for them.
        '';
      };
    };

    fonts = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Advertise the server-owned Font protocol.";
      };

      allowExport = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Permit authenticated clients to fetch font bytes. OS/2 embedding
          restrictions still take precedence. Catalogue metadata remains
          available when <option>fonts.enable</option> is true.
        '';
      };

      dirs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [
          "/usr/share/fonts"
          "/home/alice/.local/share/fonts"
        ];
        description = "Extra directories searched by each yas server's Font catalogue.";
      };
    };

    relay = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Advertise the server-owned Relay protocol.";
      };

      remoteFiles = mkOption {
        type = types.attrsOf types.str;
        default = { };
        example = {
          alice = "/run/secrets/yas-alice-remotes";
        };
        description = ''
          Optional <literal>yas.remotes</literal> path for each user in
          <option>services.yas.users</option>. Values are strings so secret
          connector credentials are not copied into the Nix store. A user
          without an entry uses the normal per-user configuration path.
        '';
      };
    };

    x11 = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Run X11 applications in yas sessions, through
          <literal>xwayland-satellite</literal>. yas's compositor speaks
          Wayland only, so without a bridge an X11-only application does not
          fall back — it fails to start, and a toolkit that can do both is
          told to use Wayland.

          The bridge is started per session only when its binary is on the
          server's PATH, which is what this option arranges; the server
          itself never requires it. Turning this off (or setting
          <option>YAS_XWAYLAND=0</option>) keeps Xwayland out of the
          closure and leaves sessions Wayland-only.
        '';
      };

      package = mkOption {
        type = types.package;
        default = pkgs.xwayland-satellite;
        defaultText = "pkgs.xwayland-satellite";
        description = "The X11 bridge to put on the server's PATH.";
      };
    };

    audio = {
      enable = mkEnableOption "audio forwarding (PipeWire capture + Opus)";

      bitrate = mkOption {
        type = types.int;
        default = 64000;
        description = "Opus encoder bitrate in bits/sec.";
      };

      realtime = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable RTKit, so yas's private PipeWire graph can run its data
          loop at realtime priority.

          PipeWire asks for <literal>SCHED_FIFO</literal> and carries on
          without it, silently, so the failure looks like nothing at all
          until the machine is busy. A desktop session normally supplies
          the privilege through RTKit; a server started from a socket unit
          or a development shell has no session, so the audio loop ends up
          on <literal>SCHED_OTHER</literal> at priority 0 and competes with
          the compositor and the video encoders it shares a machine with.
          It then misses its cycle deadline exactly when there is most to
          do — scrolling a window, resizing, anything that saturates a core
          — and the gap is cut into the captured audio itself, before any
          of it is encoded or sent. No client-side jitter buffer can
          recover audio that was never captured.

          RTKit rather than a raised <literal>rtprio</literal> limit on the
          unit: rlimits are inherited, and the server spawns the user's
          shells, so a limit here would hand every process started from a
          terminal the same ceiling. RTKit grants the priority per thread,
          to the process that asks, and polices what it hands out — and
          being a system service it also covers a server run by hand
          outside systemd, which an rlimit on the unit would not.

          Sets <option>security.rtkit.enable</option>; turn this off if you
          manage realtime privileges yourself.
        '';
      };
    };

    gpuLibraries = mkOption {
      type = types.listOf types.package;
      default = lib.optionals pkgs.stdenv.hostPlatform.isLinux [
        pkgs.libva
        pkgs.libgbm
        pkgs.vulkan-loader
      ];
      defaultText = "[ pkgs.libva pkgs.libgbm pkgs.vulkan-loader ] (Linux only)";
      description = ''
        Libraries to make available to yas server via LD_LIBRARY_PATH
        for hardware-accelerated video decoding/encoding and GPU compositing.
        yas server loads VA-API, Vulkan, and GBM via dlopen at
        runtime; on NixOS these shared objects are not in the default
        search path.

        Set to an empty list to disable hardware acceleration and use
        only software encoders (openh264, rav1e).
      '';
    };

    edges = mkOption {
      type = types.attrsOf (
        types.submodule {
          options = {
            port = mkOption {
              type = types.port;
              default = 3264;
              description = "Port to listen on.";
            };
            addr = mkOption {
              type = types.str;
              default = "127.0.0.1";
              example = "::1";
              description = ''
                Address to bind to. Loopback by default: the listener is
                plaintext `ws://` and its passphrase is full authority over the
                home server, so reaching it from the network is something a
                deployment opts into by putting a TLS reverse proxy in front.
                Set `::1` when the proxy dials IPv6 loopback.
              '';
            };
            passFile = mkOption {
              type = types.path;
              description = ''
                File containing <literal>YAS_PASSPHRASE=&lt;passphrase&gt;</literal>
                or an Argon2 PHC hash of one. Use
                <literal>YAS_EDGE_PASSPHRASE</literal> instead when the same
                server also publishes a share, so the two have a secret each.
              '';
            };
            trustedProxyIps = mkOption {
              type = types.listOf types.str;
              default = [ ];
              example = [
                "127.0.0.1"
                "::1"
              ];
              description = ''
                Exact reverse-proxy IP addresses allowed to supply the bounded
                X-Forwarded-For chain used for edge authentication throttling.
                Forwarding headers are ignored when this list is empty.
              '';
            };
          };
        }
      );
      default = { };
      example = lib.literalExpression ''{ alice.passFile = "/run/secrets/yas-alice-edge.env"; }'';
      description = ''
        Browser edges, keyed by the user whose server hosts them. The server
        serves the authenticated <literal>/edge</literal> WebSocket itself, so
        each user in <option>users</option> may have one.
      '';
    };

    shares = mkOption {
      type = types.attrsOf (
        types.submodule {
          options = {
            passFile = mkOption {
              type = types.path;
              description = ''
                File containing <literal>YAS_PASSPHRASE=&lt;passphrase&gt;</literal>.
                Use <literal>YAS_SHARE_PASSPHRASE</literal> instead when the
                same server also serves an edge, so the two have a secret each.
              '';
            };
            hub = mkOption {
              type = types.nullOr types.str;
              default = null;
              description = "Signaling hub URL. Defaults to wss://yas.run.";
            };
            quiet = mkOption {
              type = types.bool;
              default = true;
              description = ''
                Keep the sharing URL out of the log. It contains the
                passphrase, and the log is the journal.
              '';
            };
            verbose = mkOption {
              type = types.bool;
              default = false;
              description = "Print detailed connection diagnostics to the log.";
            };
            verboseWebrtc = mkOption {
              type = types.bool;
              default = false;
              description = "Enable WebRTC-level tracing (YAS_WEBRTC_VERBOSE=1): ICE candidates, STUN/TURN results, SDP exchange, and DataChannel events.";
            };
          };
        }
      );
      default = { };
      example = lib.literalExpression ''{ alice.passFile = "/run/secrets/yas-alice-share.env"; }'';
      description = ''
        WebRTC shares, keyed by the user whose server publishes them. The
        server serves each consumer itself, so each user in
        <option>users</option> may have one.
      '';
    };
  };

  config = mkIf cfg.enable {
    assertions =
      lib.mapAttrsToList (user: _: {
        assertion = lib.elem user cfg.users;
        message = "services.yas.edges.${user} names no server: an edge is served by that user's own server, so the key must be one of services.yas.users.";
      }) cfg.edges
      ++ lib.mapAttrsToList (user: _: {
        assertion = lib.elem user cfg.users;
        message = "services.yas.shares.${user} names no server: a share is published by that user's own server, so the key must be one of services.yas.users.";
      }) cfg.shares;

    # PipeWire's data loop is worthless at SCHED_OTHER on a machine that also
    # encodes video: it misses its cycle and the gap lands in the captured
    # audio. `mkDefault` so a host that manages realtime privileges its own
    # way keeps the last word.
    security.rtkit.enable = lib.mkIf (cfg.audio.enable && cfg.audio.realtime) (lib.mkDefault true);

    systemd.services = builtins.listToAttrs (
      map (user: {
        name = "yas-server@${user}";
        value = {
          description = "yas terminal multiplexer for ${user}";
          wantedBy = [ "multi-user.target" ];
          # Audio spawns pipewire / wireplumber / dbus-daemon by name,
          # so they need to be on $PATH.  Language servers likewise are
          # spawned by name and discovered via PATH (docs/design/lsp.md).
          # Use systemd.services.*.path (which prepends to the default
          # PATH) rather than overriding $PATH in Environment — that
          # would clobber coreutils and friends for PTY shells, which
          # inherit the service env.
          path =
            lib.optionals cfg.audio.enable [
              pkgs.pipewire
              pkgs.wireplumber
              pkgs.dbus
            ]
            # The portal frontend is spawned by name for yas's private bus
            # (crates/server/src/desktop_bus.rs), and it is unconditional:
            # the camera and ScreenCast portals are how Firefox and
            # Chromium find a viewer's camera and answer getDisplayMedia,
            # neither of which has anything to do with audio forwarding.
            # This package ships no bin/, so the libexec directory has to
            # go on PATH directly — listing the package would add an empty
            # bin/ and the frontend would silently never start.
            ++ lib.optional pkgs.stdenv.hostPlatform.isLinux "${pkgs.xdg-desktop-portal}/libexec"
            # Spawned by name, once per session, and only if it is here:
            # see crates/server/src/xwayland.rs.
            ++ lib.optional (pkgs.stdenv.hostPlatform.isLinux && cfg.x11.enable) cfg.x11.package
            ++ cfg.languageServers
            # Whatever the extensions on this server shell out to.
            ++ cfg.extensions.path
            # A system service does not source /etc/profile, but exact-argv
            # terminals and extensions still need the user's
            # normal Nix profiles. In particular, muster can find `direnv`
            # in the per-user profile and `nix` in the default profile before
            # entering a checkout's flake environment.
            ++ userProfileRoots user;
          serviceConfig = {
            Type = "notify";
            User = user;
            WorkingDirectory = "~";
            ExecStart = "${cfg.package}/bin/yas server";
            RuntimeDirectory = "yas/${user}";
            RuntimeDirectoryMode = "0700";
            Restart = "on-failure";
            RestartSec = "1s";
            # Let PipeWire's module-rt put the graph thread on SCHED_FIFO.
            #
            # The audio graph runs a 21 ms cycle and shares this host with
            # video encoding. Without an RT budget module-rt cannot raise
            # the thread and falls back to nice — which RLIMIT_NICE of 0
            # also refuses — so `data-loop.0` runs SCHED_OTHER at nice 0
            # against the encoder. It then misses cycles under load and
            # emits audio in bursts: measured 60-110 ms holes with nothing
            # else on the wire, which no jitter buffer sized for a 20 ms
            # cadence can absorb. RTKit is the other route and does not
            # reach this graph, whose PipeWire runs a stripped config.
            LimitRTPRIO = 95;
            LimitNICE = "-11";
            Environment =
              lib.optional (cfg.shell != null) "SHELL=${cfg.shell}"
              ++ [
                "YAS_SCROLLBACK=${toString cfg.scrollback}"
              ]
              ++ lib.optional (userDataDirs user != "") "XDG_DATA_DIRS=${userDataDirs user}"
              ++ lib.optional (serverLibSearchPath != "") "LD_LIBRARY_PATH=${serverLibSearchPath}"
              ++ lib.optionals cfg.audio.enable [
                "YAS_AUDIO=1"
                "YAS_AUDIO_BITRATE=${toString cfg.audio.bitrate}"
              ]
              ++ lib.optional (!cfg.audio.enable) "YAS_AUDIO=0"
              ++ lib.optional (!cfg.extensions.persistent) "YAS_ALLOW_EXT_PERSIST=0"
              ++ lib.optional (!cfg.fonts.enable) "YAS_FONTS=0"
              ++ lib.optional cfg.fonts.allowExport "YAS_FONT_EXPORT=1"
              ++ lib.optional (cfg.fonts.dirs != [ ]) "YAS_FONT_DIRS=${lib.concatStringsSep ":" cfg.fonts.dirs}"
              ++ lib.optional (!cfg.relay.enable) "YAS_RELAY=0"
              ++ lib.optional (lib.hasAttr user cfg.relay.remoteFiles) "YAS_REMOTES=${cfg.relay.remoteFiles.${user}}"
              ++ [ "YAS_SOCK=/run/yas/${user}/yas-default.sock" ]
              ++ hostedEnvFor user;
          }
          // lib.optionalAttrs (hostedPassFilesFor user != [ ]) {
            EnvironmentFile = hostedPassFilesFor user;
          };
        };
      }) cfg.users
    );

  };
}
