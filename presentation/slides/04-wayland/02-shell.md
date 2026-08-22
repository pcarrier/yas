# Real applications expect a real compositor

- **Speak modern Wayland:** `wl_compositor` v6, `xdg-shell` v6, decorations, viewporter, fractional scale, presentation, activation, cursor shape.
- **Expose real input protocols:** relative pointer, constraints, text input, primary selection, data device, multitouch.
- **Manage windows normally:** configure/ack, resize, maximize/restore, fullscreen/unfullscreen, parents; minimize is deliberately a no-op.
- **Make popups behave:** position/reposition, nested grabs, keyboard focus, outside dismissal, popup/subsurface hit-testing.
- **Parent portal prompts correctly:** `xdg-foreign` handles for access and screencast dialogs.
- **Run real software:** native Wayland; optional `xwayland-satellite`; Chrome, Electron, mpv.
