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
  # The edge and the share run inside the server agent: one job to keep alive,
  # and a browser or WebRTC consumer that reaches the terminals without a
  # socket in between. A share served this way also does without the yas-proxy
  # daemon, which exists to pool the socket connections it no longer makes.
  hostedPassFiles = lib.unique (
    lib.optional cfg.edge.enable cfg.edge.passFile ++ lib.optional cfg.share.enable cfg.share.passFile
  );
  # launchd hands a job an environment, not a shell, so a secret in a file is
  # read the only way a job can read one: by sourcing it before exec.
  hostedSource = lib.concatMapStrings (file: "set -a; . ${file}; set +a; ") hostedPassFiles;
  hostPort = addr: port: "${if lib.hasInfix ":" addr then "[${addr}]" else addr}:${toString port}";
  hostedEnv =
    lib.optionalAttrs cfg.edge.enable (
      {
        YAS_EDGE = "1";
        YAS_ADDR = hostPort cfg.edge.addr cfg.edge.port;
      }
      // lib.optionalAttrs (cfg.edge.trustedProxyIps != [ ]) {
        YAS_TRUSTED_PROXY_IPS = lib.concatStringsSep "," cfg.edge.trustedProxyIps;
      }
      // lib.optionalAttrs cfg.edge.webTransport.enable (
        {
          YAS_WEBTRANSPORT = "1";
          YAS_WEBTRANSPORT_ADDR = hostPort cfg.edge.webTransport.addr cfg.edge.webTransport.port;
          YAS_WEBTRANSPORT_PUBLIC_PORT = toString cfg.edge.webTransport.publicPort;
        }
        // lib.optionalAttrs (cfg.edge.webTransport.certificateFile != null) {
          YAS_WEBTRANSPORT_CERT = cfg.edge.webTransport.certificateFile;
          YAS_WEBTRANSPORT_KEY = cfg.edge.webTransport.keyFile;
        }
        // lib.optionalAttrs cfg.edge.webTransport.pinCertificate {
          YAS_WEBTRANSPORT_PIN_CERT = "1";
        }
      )
    )
    // lib.optionalAttrs cfg.share.enable (
      {
        YAS_SHARE = "1";
      }
      // lib.optionalAttrs (!cfg.share.quiet) { YAS_SHARE_QUIET = "0"; }
      // lib.optionalAttrs cfg.share.verbose { YAS_SHARE_VERBOSE = "1"; }
      // lib.optionalAttrs (cfg.share.hub != null) { YAS_HUB = cfg.share.hub; }
      // lib.optionalAttrs cfg.share.verboseWebrtc { YAS_WEBRTC_VERBOSE = "1"; }
    );
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

    shell = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/current-system/sw/bin/fish";
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
        and their binaries are prepended to the server process's PATH.
        yas matches them to files by project marker and extension, keeps
        them warm across connections, and never downloads anything. Empty
        by default. Set <option>YAS_LSP=0</option> via the environment to
        disable the family entirely.
      '';
    };

    audio = {
      enable = mkEnableOption "audio forwarding (Linux only — no-op on Darwin)";

      bitrate = mkOption {
        type = types.int;
        default = 64000;
        description = "Opus encoder bitrate in bits/sec. Only effective on Linux.";
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
          "/Library/Fonts"
          "~/Library/Fonts"
        ];
        description = "Extra directories searched by the yas server's Font catalogue.";
      };
    };

    relay = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = "Advertise the server-owned Relay protocol.";
      };

      remoteFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "/run/secrets/yas.remotes";
        description = ''
          Optional server-owned <literal>yas.remotes</literal> path. This is
          a string so connector credentials are not copied into the Nix store.
          When unset, the server uses the normal per-user configuration path.
        '';
      };
    };

    socketPath = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Unix socket path for yas server. Defaults below a private owner-only runtime directory.";
    };

    edge = {
      enable = mkEnableOption "the browser edge, served by this machine's yas server";

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
          Address to bind to. Loopback by default: the listener is plaintext
          `ws://` and its passphrase is full authority over the server, so
          reaching it from the network is something a deployment opts into by
          putting a TLS reverse proxy in front. Set `::1` when the proxy dials
          IPv6 loopback.
        '';
      };

      passFile = mkOption {
        type = types.path;
        description = ''
          File containing <literal>YAS_PASSPHRASE=&lt;passphrase&gt;</literal>
          or an Argon2 PHC hash of one. Use
          <literal>YAS_EDGE_PASSPHRASE</literal> instead when the server also
          publishes a share, so the two have a secret each.
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

      webTransport = {
        enable = mkEnableOption "the edge's native WebTransport datagram path";

        port = mkOption {
          type = types.port;
          default = 3264;
          description = "UDP port on which the WebTransport edge listens.";
        };

        addr = mkOption {
          type = types.str;
          default = "127.0.0.1";
          description = "Address on which the WebTransport UDP listener binds.";
        };

        publicPort = mkOption {
          type = types.port;
          default = 3264;
          description = "Public UDP port advertised to the browser.";
        };

        certificateFile = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "PEM certificate chain for the WebTransport TLS listener. When null, yas generates a short-lived pinned development certificate.";
        };

        keyFile = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "PEM private key paired with certificateFile.";
        };

        pinCertificate = mkOption {
          type = types.bool;
          default = false;
          description = "Advertise the exact certificate hash to browsers; the certificate must satisfy browser WebTransport hash validity limits.";
        };
      };
    };

    share = {
      enable = mkEnableOption "publishing this machine's yas server over WebRTC";

      passFile = mkOption {
        type = types.path;
        description = ''
          File containing <literal>YAS_PASSPHRASE=&lt;passphrase&gt;</literal>.
          Use <literal>YAS_SHARE_PASSPHRASE</literal> instead when the server
          also serves an edge, so the two have a secret each.
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
          Keep the sharing URL out of the log. It contains the passphrase, and
          the log is a file in /tmp.
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
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion =
          (cfg.edge.webTransport.certificateFile == null) == (cfg.edge.webTransport.keyFile == null);
        message = "services.yas.edge.webTransport.certificateFile and keyFile must be set together";
      }
    ];

    launchd.user.agents = {
      yas = {
        serviceConfig = {
          Label = "com.yas.server";
          ProgramArguments = [
            "/bin/sh"
            "-lc"
            # Language servers on PATH so `yas lsp` can discover them
            # (docs/design/lsp.md); prepended so a user-installed server
            # does not shadow the pinned one.
            (
              # launchd does not expand `~` in WorkingDirectory — it fails the
              # spawn with EX_CONFIG (78) before the job ever runs, so cd here
              # instead.
              ''cd "$HOME" || exit 1; ''
              + ''[ -n "$LANG" ] || export LANG="$(defaults read -g AppleLocale 2>/dev/null | sed 's/@.*//' || echo en_US).UTF-8"; ''
              + lib.optionalString (
                cfg.languageServers != [ ]
              ) ''export PATH="${lib.makeBinPath cfg.languageServers}:$PATH"; ''
              + hostedSource
              + "exec ${cfg.package}/bin/yas server"
            )
          ];
          EnvironmentVariables = {
            YAS_SCROLLBACK = toString cfg.scrollback;
          }
          // lib.optionalAttrs (!cfg.fonts.enable) {
            YAS_FONTS = "0";
          }
          // lib.optionalAttrs cfg.fonts.allowExport {
            YAS_FONT_EXPORT = "1";
          }
          // lib.optionalAttrs (cfg.fonts.dirs != [ ]) {
            YAS_FONT_DIRS = lib.concatStringsSep ":" cfg.fonts.dirs;
          }
          // lib.optionalAttrs (!cfg.relay.enable) {
            YAS_RELAY = "0";
          }
          // lib.optionalAttrs (cfg.relay.remoteFile != null) {
            YAS_REMOTES = cfg.relay.remoteFile;
          }
          // lib.optionalAttrs (cfg.socketPath != null) {
            YAS_SOCK = cfg.socketPath;
          }
          // lib.optionalAttrs (cfg.shell != null) {
            SHELL = cfg.shell;
          }
          // hostedEnv;
          RunAtLoad = true;
          KeepAlive = true;
          StandardOutPath = "/tmp/yas-server.log";
          StandardErrorPath = "/tmp/yas-server.log";
        };
      };
    };
  };
}
