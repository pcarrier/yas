# One PTY, many views

- **Fit this device:** rows, columns, scroll, focus, FPS, display metrics, queue target; open/configure/reset/close independently.
- **Share the PTY, not the viewport:** one client can use a larger grid without sharing scroll or focus.
- **Recover instantly:** self-contained keyframes, consecutive or explicit-base deltas, and final state.
- **Measure reality:** presented sequence, paint backlog, slots, RTT, goodput, jitter, byte/frame windows.
- **Prioritize attention:** the focused view wins; previews use spare budget; bulk cannot monopolize the link.
- **Hide safe latency:** predict local echo only when PTY echo and canonical mode make it correct.
