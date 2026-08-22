# Extensions use YAS too

- Run Wasmi or QuickJS modules addressed immutably by BLAKE3.
- Stage, verify, and atomically commit objects; declare the desired deployment.
- Set runtime, argv, limits, restart/backoff, persistence, and enabled state.
- Give every attempt a complete in-process YAS session—not privileged side APIs.
- Follow stdout/stderr/log with replay and gaps; send commands through Channel listeners.
- Durable deploy/control deduplication prevents duplicate work across restart.
