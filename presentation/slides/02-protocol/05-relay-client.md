# A route changes the machine, not the tool

- Save `rabbit`, `prod`, or `local:work`; local instances isolate their socket and state. Pick a default or override one command with `--on`.
- SSH routes auto-install YAS when needed and use the agent, key files, common `~/.ssh/config` fields, and TOFU `known_hosts` checks.
- Relay state publishes names, labels, availability, and transport hints—not connector URIs, keys, or passphrases.
- `CONNECT` opens a complete nested YAS session; early preface + `HELLO` data can save a startup flight.
- Home forwards bytes and half-closes without parsing frames, translating handles, or merging catalogues.
- Reach local, SSH, TCP, WebSocket, WebTransport, WebRTC, Uplink, or another Relay; every hop negotiates independently.
