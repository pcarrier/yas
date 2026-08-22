# LSP without leaking LSP

- The server is the sole LSP client; JSON-RPC IDs and UTF-16 positions never leak across the wire.
- Open from a Filesystem root, platform path, or terminal CWD; choose a profile or auto-discover.
- Watch backends, progress, RSS, capabilities, diagnostics, and shared unsaved buffers.
- Query definition, reference, hover, symbol, completion, action, format, rename, and signature as typed data.
- Use zero-based UTF-8 byte positions; every answer names its document revision and hash.
- Update buffers with CAS/staged UTF-8; rename returns a plan that Filesystem commits.
