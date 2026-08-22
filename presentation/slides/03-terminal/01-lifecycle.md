# A terminal is a resource, not a connection

- **Find it:** ID, tag, live title, command, CWD, size, used rows, generation, journal cursor, deadline, app handle, exit.
- **Start it your way:** shell/argv/string; server/path/terminal CWD; inherited or empty environment + overrides; size/tag/deadline/app endpoint.
- **Restart cleanly:** replay the stored launch or replace it; generation cutover rejects stale output.
- **Stay in control:** signal the group, set a deadline/dead-man switch, close, wait for exit or a pattern.
- **Keep the evidence:** exited terminals remain listable, inspectable, and restartable.
- **Go native:** attach a local raw TTY or record a timestamped `YASREC1` session.
