# Negotiate once, then be boring

- Client and server agree once on versions, codecs, limits, and platform.
- Every accepted request gets exactly one matching result—no ambiguous outcomes.
- Stable family and message-kind IDs keep traces, captures, and logs readable.
- One receive budget bounds state, frames, transfers, and reassembly.
- A faster datagram lane is negotiated only alongside the reliable session.
- `PING` measures RTT/clock offset; `CANCEL`, `GOAWAY`, and orderly `SHUTDOWN` have explicit completion rules.
