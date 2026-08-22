# Workspace layout: sway-like containers

- **Status:** Implemented
- **Date:** 2026-09-01
- **Scope:** Workspace tabs, layout containers, parked windows, keyboard,
  pointer, and touch behavior

## Decision

Use a sway-like container tree for the main workspace, controlled through the
existing `Ctrl+B` leader and ordinary arrow keys.

The browser is not a Unix display server, so parity means that the same user
intent has the same result: focus, split, move, resize, change container
layout, fullscreen, switch workspace, park, restore, and close. Browser-only
resources such as editor and web views participate as normal containers.

The right sidebar remains a first-class shelf. It is the visual equivalent of
sway's scratchpad, made persistent and discoverable instead of hidden behind a
cycle command.

## Model

A workspace contains:

- a durable workspace identity and attached Relay routes;
- one recursive container tree for visible work;
- a shelf of live, parked views in the right sidebar;
- client-local focus, fullscreen state, and responsive presentation.

A leaf container owns one terminal, Wayland toplevel, editor, diff, commit,
management view, or web view. Interior containers use one of four layouts:

- **horizontal:** children divide width;
- **vertical:** children divide height;
- **tabbed:** one child is visible behind a horizontal title strip;
- **stacking:** one child is visible behind vertically stacked title bars.

Horizontal and vertical containers may nest without restriction. Adding a
child to a container with the same axis extends that container; it does not
create a redundant two-child wrapper. A different axis creates a nested
container. This is the behavior that makes sway layouts predictable rather
than a growing BSP staircase.

Pane identifiers are tree paths and may change after an edit. Assignments are
therefore re-keyed by leaf identity in the same atomic update as the tree.
Terminals and surfaces never briefly acquire two visual owners, disappear, or
receive a transient 1×1 resize during a transformation.

## Workspace tabs

Workspace tabs are durable shared workspaces. Attaching a tab is client-local;
its resources and layout are shared. Active workspace and container tabs use
reversed foreground/background colors.

| Shared by a workspace             | Local to one client                    |
| --------------------------------- | -------------------------------------- |
| Container tree and ratios         | Focused container                      |
| Visible and parked view ownership | Active tab/stack child                 |
| Workspace name and routes         | Fullscreen presentation                |
| Resource lifecycle                | Attached-tab order and selected tab    |
| Failed/reconnecting placeholders  | Responsive folding and physical chrome |

Detach never closes a resource or changes the shared layout. Deleting a
workspace remains an explicit management action with no default shortcut.

## Core behavior

### Focus

Directional focus uses rendered geometry, not a guess based on tree order.
Candidates in the requested half-plane are ranked by perpendicular overlap,
edge distance, perpendicular distance, recent focus, and stable render order.

Tabbed and stacking containers keep every child mounted. Selecting a hidden
child changes its local active child and restores its existing terminal,
editor, or web state.

Each header follows its content's live title: terminal title/command, Wayland
window title, editor filename, commit, management view, or web host. Numbered
labels are used only while that identity is unresolved.

### Splitting and opening

`Ctrl+B h` and `Ctrl+B v` set horizontal or vertical intent for the next open.
The tabbed and stacking commands do the same from a single-pane workspace. A
small layout indicator stays visible until the intent is consumed.

`Ctrl+B Enter` opens a terminal as a sibling in the focused container. It uses
the container's current layout. In a horizontal or vertical container it tiles;
it becomes a tab only when the user has explicitly made that container tabbed.

`Ctrl+B Shift+Enter` opens a terminal in a new container. It consumes explicit
split intent when present; otherwise it chooses the trailing side of the
focused container's longer axis. Resource creation completes before the tree
changes, so a failed terminal start never leaves an empty structural pane.

Ordinary file, web, terminal, and toplevel opens inherit the focused container's
layout. Starting from the standalone view creates a horizontal split, never an
implicit tab group. Explicit open-beside actions use the same populated split
operation as terminals.

### Container layouts

The focused container's parent may change between horizontal, vertical,
tabbed, and stacking without changing child identity or order.

- Split toggle changes horizontal to vertical and every other tiled layout to
  horizontal.
- Tabbed and stacking are explicit; opening or moving never manufactures them
  as a side effect.
- Layout cycle walks horizontal, vertical, tabbed, stacking.
- Balance gives every child of every horizontal/vertical split equal weight.

### Moving

Directional movement changes the tree:

- adjacent siblings on the requested axis exchange positions;
- across nested containers, the source is removed and inserted beside the
  geometric target;
- at an outer edge, the source becomes a new leading or trailing root child;
- empty ancestors collapse;
- the moved resource remains focused.

This is container movement, not assignment swapping and not implicit tab
grouping. Dropping a card on a container's center is still an explicit pointer
operation for grouping it as a tab.

### Resizing

Directional resize moves the nearest boundary on the matching axis. Divider
drag converts pointer movement within the split into adjacent weights so the
boundary follows the pointer regardless of sibling count or weights. At an
outer edge or minimum weight, resize is a no-op. Balance recursively resets
tiled child weights.

Surface panes remeasure on the next animation frame after changes to the
layout tree, including split weights, orientation, and floating rectangles,
or to the font size that controls window chrome. These measurements recover
missed ResizeObserver delivery without recreating the surface stream.
Unchanged visibility preserves the resize driver and its size claim throughout
these updates; only hiding the pane releases that claim.

### Fullscreen

Fullscreen hides sibling branches without rewriting or unmounting the tree.
Terminal and surface sizing ignore hidden siblings. Surface views explicitly
measure their new box on the next animation frame when fullscreen changes,
without depending solely on ResizeObserver or recreating the stream. Leaving
fullscreen restores the exact ratios and child state.

### Parked windows and the right sidebar

Parking is the safe alternative to closing:

- the view leaves the visible tree;
- its process, document, or surface remains alive;
- the right sidebar shows its live card or preview;
- selecting or dragging the card restores it;
- restoring to a container center groups it deliberately;
- closing from the sidebar still performs the resource-specific close action.

The sidebar is never absorbed into, occluded by, or resized with the container
tree. Hiding it changes only client chrome; parked resources remain recoverable
from the overview and reopen when the sidebar returns.

Development hot reloads retain the latest layout and surface arrival history,
including an empty layout, so already-parked windows stay on the shelf.
Home connections close on module disposal as well as component cleanup, so a
missed cleanup cannot leave an old browser session constraining window sizes.

Parking remembers each view's tiled or floating placement by stable resource
identity. Restoring a floating view restores its frame, including position and
size; its reference viewport preserves pixel geometry when the shelf changes
the available space. Parked placements are saved with the workspace, so they survive reloads
and detaching and reattaching the workspace.

Closing a Wayland surface sends a close request to its application. Its pane,
focus, and parked card stay in place until the server catalogue removes the
surface, even after the request is acknowledged. Applications can show a
confirmation dialog or decline to close without losing their view.

## Default keyboard map

`Ctrl+B` is the only reserved chord. It arms one command and immediately shows
the current map. `Ctrl+B Ctrl+B` forwards a literal `Ctrl+B`; `Escape` cancels.
On Apple platforms the leader is also available as `Command+B`.

### Containers

| After `Ctrl+B`      | Action                                      |
| ------------------- | ------------------------------------------- |
| `Arrow`             | Focus in that direction                     |
| `Shift+Arrow`       | Move the focused container                  |
| `Alt+Arrow`         | Resize the nearest boundary                 |
| `h` / `v`           | Set next split horizontal / vertical        |
| `b`                 | Toggle horizontal / vertical container      |
| `t` / `s`           | Set tabbed / stacking container layout      |
| `Space`             | Cycle container layout                      |
| `1`…`9`             | Focus a visible container by map order      |
| `Tab` / `Shift+Tab` | Next / previous view                        |
| `z`                 | Toggle fullscreen for the focused container |
| `=`                 | Balance all tiled splits                    |
| `q`                 | Park the focused view in the right sidebar  |
| `x`                 | Close the focused view                      |

### Opening and navigation

| After `Ctrl+B`  | Action                                      |
| --------------- | ------------------------------------------- |
| `Enter`         | New terminal in the focused container       |
| `Shift+Enter`   | New terminal in a new container             |
| `k`             | Open the command menu                       |
| `/`             | Search workspace commands                   |
| `w`             | Open the view overview and shelf            |
| `r`             | Show/hide the parked-window right sidebar   |
| `[` / `]`       | Previous / next attached workspace tab      |
| `n` / `a` / `d` | Create / attach / detach a workspace tab    |
| `e f y l p`     | Explorer / search / branches / log / issues |

All direction commands use physical arrow keys. Vim direction letters are not
aliases, so terminal applications retain them and the map remains legible on
non-US keyboard layouts.

## Pointer and touch

- Clicking a visible container focuses it.
- Dragging a divider resizes its adjacent children.
- Dropping on a center explicitly groups as a tab.
- Dropping on an edge creates a populated split in that direction.
- Dragging a tab header to an edge extracts it into a split, including the
  currently active tab.
- Dragging to or selecting the right sidebar parks/restores without closing.
- Drag, float/tile, solo/restore, park, and close for the focused pane live on the
  right of the top tab bar, beside the workspace session controls.
- The top tab bar and bottom status bar share height, text size, and control
  sizing. Icons and button widths scale to 1.75× on touch displays; text stays
  at the workspace font size. The bottom safe-area inset sits below the status
  bar without changing its height.

On narrow screens, presentation may show the focused container alone, but it
does not rewrite the durable desktop tree. Returning to a larger viewport
restores the same layout and weights.

## Layout representation

There is one tiled workspace with optional floating windows, without a global
layout-mode switch. Layouts are stored and synced as `{name, root}` JSON objects.
The root is the same structured container tree used by the renderer; a workspace
container holds an optional tiled base and floating children with explicit frames.
There is no layout language, parser, or migration from text layouts.

## Invariants

- A resource has at most one visible owner.
- A tree edit and assignment re-key publish atomically.
- Moving and resizing never close a resource.
- Parking never stops a process or discards a document.
- Closing acts on the resource, not merely its rectangle.
- Split creation is populated or does not happen.
- Focus is local to a client; layout edits are shared.
- Responsive presentation never mutates the saved tree.
- The right sidebar remains available as the recovery surface for parked work.
