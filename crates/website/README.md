# yas.run

One Rust process serves the embedded `js/web` build, installer scripts, and
the WebRTC signaling API. Redis holds presence and relays signaling messages
between the two Fly Machines.

`/` serves the landing page only when `Accept` includes `text/html`; otherwise
it serves the PowerShell or shell installer based on the user agent.

## First deployment

Use the `yas-887` Fly organization throughout:

```sh
fly apps create yas-run --org yas-887
fly redis create --name yas-run-redis --org yas-887 --region cdg --no-replicas
fly secrets set REDIS_URL='redis://…' -a yas-run
fly certs add yas.run -a yas-run
fly deploy . --config crates/website/fly.toml
fly scale count 2 --region cdg -a yas-run
```

The Redis creation command prints the private `REDIS_URL`; set that value as
the app secret. Optional Cloudflare TURN credentials are `CF_TURN_TOKEN_ID`
and `CF_TURN_API_TOKEN`.

Point the apex `yas.run` records at the addresses shown by
`fly ips list -a yas-run`. GitHub Actions deploys with the personal account's
`FLY_API_TOKEN` repository secret.
