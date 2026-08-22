# Large payloads get flow control, not special cases

- File bodies, query pages, stdio, relay tunnels, font faces, clipboard data, and channel messages share one bounded bulk-data mechanism.
- Byte/message mode, exact offsets, boundaries, credit, close, and reset stay explicit.
- Each feature owns the meaning: Filesystem validates writes; Process maps stdout; Network owns sockets.
- Closing the stream never installs a file—the owning feature must validate and commit it.
- Sensitive payloads are marked so routine diagnostics do not log them; confidentiality still comes from the transport.
