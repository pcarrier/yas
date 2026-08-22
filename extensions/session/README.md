# `session`

A durable list of GUI applications the session brings up and keeps up. Enable an
application once and it starts with the session and comes back when it dies.

```bash
yas ext run --persist --restart always session extensions/dist/session.wasm
yas @session list
yas @session enable legcord
yas @session status legcord
yas @session disable legcord

# This session only, leaving intent alone
yas @session start legcord
yas @session stop legcord

# Off the list entirely
yas @session forget legcord
```

`enable`/`disable` are intent — what the next session start does.
`start`/`stop` are now. Trying an application is not the same as adopting it,
and stopping one for a minute is not the same as never wanting it again; one
button for both makes those indistinguishable.

`disable` keeps the row, because an application that just failed is worth
being able to look at and its failure count is the only record of that.
`forget` drops it: the stored intent is deleted rather than written "off", and
what is left is an installed application like any other.

`--persist` requires a server that permits persistent extensions — the default,
unless the operator passed `--no-persistent-extensions` — which is also what
makes the intent outlive a restart.

The intent is stored in the server KV database. Consequently servers started
with different `--name` values have independent application settings, as well
as independent installed copies of the extension itself.

## How it starts an application

Four native YAS families do the work, none of which the extension could fake:

- **Env GET** answers with the server's environment, which is the only way a
  Wasm guest can learn `XDG_DATA_DIRS` and so find installed applications at all.
  A server started from a unit inherits none of the login environment, so an
  operator has to put `XDG_DATA_DIRS` there or the roots collapse to the spec's
  `/usr/local/share:/usr/share` — absent on NixOS — plus `~/.local/share`. The
  catalog then looks populated while everything installed through a package
  manager is missing (`nix/nixos-module.nix` sets it from `environment.profiles`).
- **Surface CREATE_APP_ENDPOINT** mints a Wayland endpoint dedicated to one
  application and tells the compositor that everything arriving on it belongs
  to that application. The endpoint is bound before the reply is sent, so the
  application can be spawned immediately.
- **Process SPAWN** with the session environment and `SPAWN_DETACHABLE` supplies
  the desktop bus, audio sockets, and toolkit steering. The exact environment
  returned by Surface, including `WAYLAND_DISPLAY`, overrides the session's
  generic endpoint.
- **FS INDEX/READ** walks application and icon roots and reads desktop entries
  in bounded native query pages. It starts no shell helper and carries no
  compatibility command packets.

## Why `status` can be trusted

`windows` is counted from the `application_id` in the native Surface catalogue,
which the compositor derives from the endpoint it stamped, not from
`xdg_toplevel.set_app_id`. The difference is not
academic: a Chromium launched under this extension reports a self-asserted
`app_id` of `claude-desktop` for a window showing `about:blank`. Anything built
on `app_id` matching files that window under the wrong application; the stamped
identity does not, because the application never gets to speak it.

## The browser panel

`yas.session.v1` publishes the whole state as JSON (one `{"type":"state"}`
object per change, with `catalog` on a greeting or a `resync`) and takes one
bare text line back: `enable`, `disable`, `start`, `stop`, `forget`,
`resync`, each followed by a desktop-entry id. `js/ui/src/session.ts` mirrors it, and the
**Applications** tab of an expanded remote is what a viewer sees.

### Icons

Artwork is the one thing the panel asks for rather than being told. `icons`
takes newline-separated ids — newline because a desktop-entry id is a filename,
and Steam alone installs hundreds with spaces in them — and is answered with one
`{"type":"icon","id":…,"path":"/…"}` per id, with `path` omitted when there
is none. The browser reads that path through native FS only when it needs the
artwork. It is a request because the asymmetry is enormous: a thousand-entry
catalog is tens of kilobytes of names and tens of megabytes of icons, so the
panel asks for the dozen rows it is drawing and nothing else.

`icon.rs` resolves an `Icon=` value the cheap way — the best-sized file of that
name anywhere on the icon path, rather than the spec's theme-inheritance search
— with bounded native FS INDEX/READ pages. Scalable art wins, then the smallest
raster at or above 128px. Results are cached in the guest by icon name, because
a desktop and its `-nightly` twin share one, and so do the dozens of entries
that all say `application-x-executable`.

## Restarting, and not restarting

Backoff mirrors the server's own extension supervisor — 250 ms base, 30 s cap,
full jitter, and a run that lasts 60 s forgives the failure history. Jitter
matters because a session starts several applications at once, so a shared cause
(a GPU reset, a compositor restart) would otherwise have them all retry in
lockstep forever.

Children are spawned `SPAWN_DETACHABLE`, so they outlive a restart of this
extension. Their opaque native Process handles are persisted beside the full
16-byte Core boot ID. A different boot ID means the server restarted and every
child went with it, so the supervisor starts clean rather than interpreting a
stale handle in a new boot.

## Testing

`cargo test --manifest-path extensions/Cargo.toml -p yas-ext-session` covers
desktop-entry field codes and quoting, icon ranking, backoff/failure rules, and
the exact native persistence record. The shipped binary is also compiled for
`wasm32-unknown-unknown` so its native family calls and guest entrypoint are checked in
the deployment target.
