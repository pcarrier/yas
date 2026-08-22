# Backend workspaces

- **Status:** Implemented
- **Storage:** home-server YAS KV (`ui/workspace-sessions/v1/`)
- **URL:** `#workspace=<id>` (`#session=<id>` remains a legacy input)

## Purpose

A workspace is the durable browser state around the processes that
YAS already keeps alive. It records the selected remotes, pane layout and
assignments, focused content, and persistent panel state. The browser attaches
to one record and detaches without deleting it; another authenticated browser
can attach to the same record later.

The steady-state URL contains only the workspace-session ID. The ID is a
locator, not an authentication secret. First-contact `psk` fragments are still
consumed and removed before the session URL is canonicalized.

## Storage and identity

Records live in the fixed home server's existing redb-backed YAS KV store,
under one key per session:

```text
ui/workspace-sessions/v1/<uuid>
```

Each browser installation also has one durable attachment-order record, keyed
by the canonical UUID kept in `yas.workspaceSessionDeviceId` local storage:

```text
ui/workspace-session-devices/v1/<device-uuid>
```

That versioned record contains only an ordered, unique, bounded list of session
IDs. It is shared by tabs with the same device UUID; the selected session is
still URL/tab-local. The ordered attachment list is rendered as the app's
top-level workspace tab strip; closing a tab detaches it from this device
without deleting the backend session.

The device UUID is created in local storage before either backend store opens.
First-use creation is serialized with Web Locks when available and an IndexedDB
read/write transaction otherwise, so simultaneous browser tabs converge on one
ID instead of creating competing device records.

This deliberately does not use Core's `session_id`, which identifies one YAS
transport connection, or the terminal `SessionId`, which is browser-local.
`WorkspaceSessionId` is an opaque random UUID. All authenticated clients of a
home server share the catalogue; the ID is not a capability.

One record contains:

- version, ID, name, and creation/update times;
- an ordered set of active Relay route names (the local home route is
  implicit);
- layout name/DSL, pane assignments, focused pane, and main focused content;
- left/preview dock visibility, expanded left sections, Explorer project,
  Muster expansion, and debug-panel state.

Transient overlays and searches are not session state. Physical chrome
geometry (dock widths), media devices, palette, font, and authentication stay
device/user scoped, so attaching the same session on a phone and a desktop
does not copy unusable dimensions or credentials between them.

Remote names are persisted, never Relay handles or generations: handles are
boot-scoped. YAS terminals and surfaces use their native u64
`(connection name, terminal_handle)` and `(connection name, surface_handle)`
identities; IDE/web panes use their server-backed tab IDs. The one-time URL
importer can recognize retired numeric URL fields, but it never turns those
browser-local values into durable native identities. An unavailable remote or
unresolved object remains in the record rather than being erased by a partial
restore.

The generic KV limits remain authoritative: keys are bounded, each document is
at most the negotiated KV value limit, the catalogue shares the home store's
entry/byte limits, and watches use the existing State/Transfer flow control.
Durable mutations fail visibly if the backend cannot commit them.

Layout DSL is parsed before a session record is accepted. The shared parser
allows at most 2,048 panes and 64 levels, so both persisted-state validation and
the UI reject flat or recursively nested layouts before constructing an
unbounded layout tree.

## Lifecycle

- **Create** writes a fresh UUID with an absent-key precondition and durable
  commit. A collision generates another UUID.
- **Attach** durably adds/reorders the ID in this device's attachment record,
  selects the catalogue record, begins applying its state, and writes
  `#workspace=<id>` with `pushState`.
- **Detach** durably removes the ID from this device's attachment record and
  stops applying the session. It does not delete the session itself. Removing
  the last ID preserves an intentionally empty device record.
- **Rename** is a semantic CAS patch; the ID and URL remain unchanged.
- **Delete** is a durable, hash-conditioned delete. Deleting the attached
  session first detaches it, and later local UI effects must never recreate it.

There is one prefix watch for the catalogue, not one watch per stored session,
plus one exact-key prefix watch for the current device record. Large records
that do not fit inline are fetched by hash/revision. A
`WorkspaceSessionAttachment` remains the local subscription handle; durable
membership lives in the companion device record rather than a stale
`attached=true` bit inside a shared session.

Direct attachment is subject to the same catalogue-entry and aggregate
retained-byte budgets as watch reconciliation; it cannot install an over-cap
record or evict an existing entry. If a previously valid exact record becomes
malformed or over-budget, the store quarantines it and retains the last-good
attachment when that record still fits. `getPresence(id)` distinguishes that
state from an actual deletion, and repair replaces the last-good value in
place.

## Concurrency

Every local change is a semantic patch over the last authoritative document.
The write uses a content-hash CAS. On conflict the client fetches the current
document, reapplies only the changed fields, and retries a bounded number of
times. Thus a panel toggle does not overwrite a concurrent rename or remote
selection. Deletion wins over an updater: a missing record is never recreated
by the retry path.

Remote route toggles use `setRemoteActive`, which reapplies one membership
change to the latest array instead of replacing the full array. Device attach,
detach, and reorder operations likewise rebase their intent, preserving
unrelated concurrent tab changes. Deleted-ID pruning confirms each session key
is absent and aborts conservatively if the device record races.

Layout resize updates are debounced, and hydration has an explicit restoring
barrier. Until remote references and pane assignments have resolved, the
browser cannot persist a partial workspace over the complete backend record.

## Bootstrap

The browser connects and authenticates the home YAS link first, opens the
workspace-session catalogue, then materializes only `local` and the attached
session's selected Relay routes. Workspace panes restore only after those
connections become available.

An absent device record means this installation has never initialized. First
bootstrap creates a candidate Default session and claims it with an
absent-key CAS; simultaneous tabs have one winner, and losers select the winner
and remove their unique orphan. An existing empty device record is intentional
and reopens the session manager without creating another Default. New sessions
start with no active remotes; home is implicit.
