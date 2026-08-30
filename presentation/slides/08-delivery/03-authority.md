# Full control is full control

- A normal session has the server OS identity: terminals, processes, environment, files, network endpoints, and every advertised Relay route.
- The browser passphrase is a bearer credential for that full control—it is not an account or viewer token.
- A WebRTC share passphrase is read-write; its derived `.ro` token asks the server for a read-only catalogue.
- Read-only guests can observe selected Terminal, Surface, Media, and Font state—never control, enumerate, or shut down.
- WebSocket beyond loopback requires WSS/TLS.
- YAS v1 has no general per-user or per-family ACL: use separate server processes/OS identities for mutually untrusted users.
- `SENSITIVE` suppresses routine payload logging; it is not encryption or authorization.
