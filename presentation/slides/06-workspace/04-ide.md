# The browser became an IDE by accident

- Explorer, Search, and Problems panels stay live from native Filesystem and LSP state.
- Editor, diff, commit, web, terminal, and Wayland-app panes share the same tiling layout and focus model.
- Saves use Filesystem compare-and-swap plus atomic commit; a stale buffer becomes a visible conflict, never a silent overwrite.
- Git supplies structured history, trees, patches, blame, reflog, and live status; the browser does not parse command output.
- LSP supplies diagnostics, navigation, hover, symbols, completion, actions, formatting, rename plans, and signatures as typed records.
- Git and LSP analyze; Filesystem commits edits; Process or Terminal handles arbitrary commands.
