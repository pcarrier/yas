# Nix modules

The modules run a YAS server. That server also serves the browser edge and
publishes the WebRTC share when you ask for them: one unit, one process, and a
browser or WebRTC consumer that reaches the terminals without a socket in
between. A share served this way needs no yas-proxy daemon either — that
daemon exists to pool the socket connections it no longer makes.

A server serving both wants a secret each: put `YAS_EDGE_PASSPHRASE` in the
edge's `passFile` and `YAS_SHARE_PASSPHRASE` in the share's. `YAS_PASSPHRASE`
still answers for both when one secret is what you want.

## nix-darwin

```nix
{ inputs, ... }: {
  imports = [ inputs.yas.darwinModules.yas ];

  services.yas = {
    enable = true;

    # Server-owned Font protocol. Bytes remain unavailable unless allowExport
    # is true and the face's embedding metadata also permits export.
    fonts = {
      enable = true;
      allowExport = false;
      dirs = [ "/Library/Fonts" ];
    };

    # Server-owned Relay catalogue. Keep this file outside the Nix store when
    # it contains SSH identities, passphrases, or other connector credentials.
    relay = {
      enable = true;
      remoteFile = "/run/secrets/yas.remotes";
    };

    edge = {
      enable = true;
      port = 3264;
      passFile = "/run/secrets/yas-edge.env";
    };

    share = {
      enable = true;
      passFile = "/run/secrets/yas-share.env";
    };
  };
}
```

See [`darwin-module.nix`](darwin-module.nix) for the full list of options.

## NixOS

```nix
{ inputs, ... }: {
  imports = [ inputs.yas.nixosModules.yas ];

  services.yas = {
    enable = true;
    users = [ "alice" "bob" ];

    fonts = {
      enable = true;
      allowExport = false;
      dirs = [ "/usr/share/fonts" ];
    };

    relay = {
      enable = true;
      remoteFiles.alice = "/run/secrets/yas-alice.remotes";
      remoteFiles.bob = "/run/secrets/yas-bob.remotes";
    };

    # Keyed by the user whose server serves them.
    edges.alice = {
      port = 3264;
      passFile = "/run/secrets/yas-alice-edge.env";
    };

    shares.alice.passFile = "/run/secrets/yas-alice-share.env";
  };
}
```

See [`nixos-module.nix`](nixos-module.nix) for the full list of options.

## Secrets and transport security

`passFile` is an environment file. It normally contains a line such as
`YAS_PASSPHRASE='secret'`; quote an Argon2 PHC value so shell metacharacters
remain literal on nix-darwin. Relay remote files use the ordinary
`name = uri` format. Pass secret paths as strings rather than importing their
contents into a Nix expression, which would copy credentials to the Nix store.
Each file must be readable by the server that reads it.

Automatic Unix sockets are owner-only. On NixOS the YAS listener for user
`alice` is `/run/yas/alice/yas-default.sock`. An edge verifies the connected
server's kernel peer UID and normally runs as the same `user`. For an
intentional explicit cross-UID socket, set the edge's `expectedServerUid` to
the server's numeric UID; this changes the required identity and never
disables the check.

The NixOS module starts each per-user server at boot and the server binds its
own listener. There is no socket unit and peer credentials identify `user`
directly. Leave `expectedServerUid` unset unless you really are pointing an
edge at another account's server.

An edge passphrase grants full authority over its home server, including every
published Relay route. The built-in edge listener is plaintext, so `addr`
defaults to `127.0.0.1` and the edge is reachable only through something you
put in front of it. Widening it is the deployment saying it has a WSS/TLS
reverse proxy; do not expose the edge as plain `ws://` on an untrusted
network.

When the reverse proxy connects from a stable address, list that exact address
in the edge's `trustedProxyIps` option so independent clients do not share one
auth-throttle key. The default is empty and ignores all forwarding headers.
The proxy must append the actual client IP to `X-Forwarded-For`; never list a
public client range as trusted.
