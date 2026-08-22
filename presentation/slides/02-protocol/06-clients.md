# Also: find the forgotten tabs

- Watch each session's client build, label, typed origin, connected time, and idle time.
- Read cumulative traffic plus sampled inbound/outbound bandwidth—never a misleading lifetime average.
- See active terminal and surface views with their dimensions, plus FS, Git, LSP, KV, and other state watches.
- Use **Connected clients** in the browser or `yas client list` from the CLI.
- Disconnect a session with a reason; it receives orderly `GOAWAY` after your result is queued.
- Client disconnect removes one session. Core `SHUTDOWN` is the separate, explicit whole-server operation.
