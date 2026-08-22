# Yes, fonts are protocol state

- Browse the server's real families, faces, styles, weights, stretch, slant, and metrics.
- Match monospace, variable, color, and fetchable faces; axes, localized names, Unicode coverage.
- Export bytes only when server policy and embedding metadata permit it.
- Deliver descriptions/faces inline or by bounded Transfer; verify with BLAKE3.
- Cache by content hash across server restarts and Relay route changes.
- Match terminal cell metrics in the browser; no hidden font side API at the edge.
