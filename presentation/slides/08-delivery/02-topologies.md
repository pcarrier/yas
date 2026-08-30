---
layout: diagram
---
# The path changes. The session does not.

```mermaid
flowchart TB
  subgraph LOCAL[Local]
    direction LR
    LB[Browser] --> LE[Embedded edge] --> LH[Home server]
  end
  subgraph REMOTE[Remote]
    direction LR
    RB[Browser] --> RH[Home server] --> RELAY[Relay] --> RS[Remote server]
  end
  subgraph SHARE[Shared]
    direction LR
    SB[Guest browser] <-->|WebRTC| SF[yas share] --> SS[Home server]
  end
```

- A proxy keeps remote routes warm, ready for the next action; an integrated `server --share` can remove the separate share process.
- The session remembers routes, panes, focus, and panel state in KV.
- Every resource handle stays scoped to the server that owns it.
