# Clipboard ownership is state too

- **Track real ownership:** clipboard, primary selection, drag/drop; multiple items/MIME forms; explicit owner, revision, lifecycle.
- **Move any size:** inline small values, bounded Transfer for large ones, atomic staged content.
- **Bridge the browser:** text and images; prefer text for rich clipboards, PNG for image-only; never truncate or serve stale data.
- **Paste in the right order:** publish selection, then release `Ctrl+V`; an empty browser clipboard preserves the Wayland owner.
- **Cross every mode:** surface ↔ surface and surface ↔ terminal even when host clipboard writes fail.
- **Drag remote files:** enter/motion/leave/drop/cancel, per-session staging, live `text/uri-list` during hover.
