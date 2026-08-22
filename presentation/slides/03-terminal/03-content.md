# Screenshots would have been much easier

- **Keep every style:** palette/default/RGB foreground/background; bold, dim, italic, underline, inverse, wide/continuation.
- **Render real text:** UTF-8 graphemes, combining sequences, wide glyphs, color emoji, overflow strings.
- **Preserve context:** cursor, live title, modes, scrollback, viewport, line flags, OSC 7 CWD.
- **Open links safely:** OSC 8 and visible-URL fallback, wrapped extents, hover, allow/confirm/deny policy.
- **Match the font:** server catalogue, exact metrics, optional face transfer, content-hash cache, palettes.
- **Use the best renderer:** WASM → zero-copy vertices → WebGPU/WebGL2/Canvas 2D, with selection/link/emoji/echo/scrollbar overlays.
