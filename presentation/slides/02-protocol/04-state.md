---
layout: diagram
---

# Reconnect is a state transition

```mermaid
flowchart LR
  WATCH[Watch] --> SNAP[Bounded snapshot]
  SNAP --> DELTA[Ordered deltas]
  DELTA --> ACK[Ack revision + credit]
  ACK --> DELTA
  RECONNECT[Reconnect with boot + revision] --> CURSOR{History retained?}
  CURSOR -->|Yes| DELTA
  CURSOR -->|No: explicit reset| SNAP
```

- Resume every watch from an explicit revision or cursor.
- Coalesce or replay while history exists; send an explicit reset and fresh snapshot when it does not.
- Retry a mutation by ID without accidentally performing it twice.
