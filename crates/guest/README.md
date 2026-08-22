# `yas-guest`

`yas-guest` is the Rust SDK for YAS Wasmi extensions. The five `yas_v1` host
imports provide bounded byte-stream transport, waiting, clocks, and entropy.
The SDK carries a normal YAS session over that stream; individual host writes
and reads are transport chunks, never YAS frame boundaries.

Use the native top-level `Client` and `entry!` for new extensions:

```ignore
use yas_guest::{Client, Error};

fn extension(mut yas: Client) -> Result<(), Error> {
    let identity = yas.context();
    let _ = identity;
    let _ = yas.ping()?;
    Ok(())
}

yas_guest::entry!(extension);
```

The exported `yas_wire_v1` marker lets the supervisor select the native YAS
endpoint before guest code runs. Bootstrap sends the preface and Core HELLO,
validates the selected Extension family and its Transfer/Channel dependency
closure, then requires sensitive `ATTEMPT_CONTEXT` as the first application
frame. The context contains the immutable extension, definition, attempt,
task, object hash, name, flags, and raw argument vector. Additional families
can be requested with `Client::bootstrap_with_offers`.

`Client::hello` and `family` expose the negotiated catalogue. Family helpers
own all peer-to-guest credit so their retained buffers share one session-wide
bound; raw credit-bearing Request and Transfer primitives stay internal to the
SDK.

The crate is `no_std` and uses `alloc`; native test shims enable `std` behind a
target gate. `register_getrandom!` installs `yas_v1.random` for the pinned
`getrandom` 0.2 custom backend, including `rand` 0.8 consumers. A crate that
exports its own entry point must expand it exactly once. For
attacker-controlled keys, use `yas_guest::collections::HashMap` or `HashSet`;
their SipHash state is keyed from host entropy.

The host accepts at most 16 MiB per stream chunk and 64 KiB per entropy request.
The safe wrappers validate sizes, split arbitrary stream writes, grow receives
only after an explicit required-size result, cap aggregate buffered YAS data,
and chunk entropy fills. Raw linear-memory pointers are never part of the
public Rust API.

The guest advertises 32 MiB of buffered receive capacity: 16 MiB is one shared
pool for committed resource credit and 16 MiB is reserved for exact decoded
frames deferred by the multiplexer. Every read keeps one maximum decoded frame
of pending headroom. A committed credit lease is reusable only after the peer
authority is explicitly retired; abandoning a live wrapper pins its lease
until session teardown. A Channel's in-memory complete-message bound is its
negotiated `max_item_bytes`, and the SDK accepts the Channel only when it can
reserve that full amount. Application messages larger than that bound are not
made safe merely by fragmenting their wire representation.
