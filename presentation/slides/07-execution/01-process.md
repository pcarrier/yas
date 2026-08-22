# Not every program wants a PTY

- Launch exact argv/environment with no implicit shell; choose server/path/terminal/FS CWD and optional app endpoint.
- Stream stdin/stdout/stderr as bounded Transfers with lifetime offsets; optionally merge stderr.
- Watch argv0, native PID, owner, lifecycle, detachable flag, offsets, exit, and retention.
- Attach at the current output offset; see an explicit gap for lost history; one stdin owner, many observers.
- Signal, terminate, kill, detach, or wait; keep the process session-owned or detachable.
- An operation ID prevents a lost result from spawning the program twice.
