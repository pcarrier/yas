# Observation must not become backpressure

- Record to a process-wide bounded binary ring with stable event IDs and typed payloads.
- Get/CAS configuration for capacity and the active event set.
- Dump hashed retained history; follow retained/live; receive an exact `GAP` when data is lost.
- Record server-side by path with append/history policy, list/stop, and final counters.
- Live observers never backpressure the work they observe.
- Raw frames, PTY bytes, environment, and content stay disabled by default.
