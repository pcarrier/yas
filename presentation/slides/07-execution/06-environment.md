# Sometimes the exact environment is the bug

- Read the server process environment plus derived values such as its effective name.
- Preserve raw platform byte-string keys/values in deterministic order.
- Return typed records inline up to 32 KiB; use bounded message Transfer above that.
- Treat it as one immutable boot snapshot: GET only, no watch or write.
- Nothing is redacted, so only full-authority sessions receive it.
