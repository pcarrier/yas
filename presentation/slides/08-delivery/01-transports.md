# The transport is replaceable

- Local: Unix sockets or Windows pipes. Remote: TCP or SSH. Browser: WebSocket, WebTransport, or WebRTC.
- One preface, one `HELLO`, the same families and resources everywhere; only the framing adapts.
- The reliable path is always authoritative.
- WebTransport and WebRTC can send eligible Surface, Media, and native Network Events as unordered, non-retransmitted datagrams.
- Surface, Media, and Network decide how to handle loss, ordering, and recovery.
- Every datagram-capable Event can fall back to the reliable path; Terminal frames always stay reliable.
