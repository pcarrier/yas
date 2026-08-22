//! Core liveness, independent of application handlers and reliable writes.

use super::*;

// Selection GET is the only other server-originated Request; its allocator
// skips this ID. Only one Ping is outstanding, so reuse follows its Result.
pub(super) const REQUEST_ID: u32 = u32::MAX;

#[derive(Default)]
pub(super) struct Heartbeat {
    pending: AtomicBool,
    replied: Notify,
}

impl Heartbeat {
    /// Consume validated Ping Results in the reader, before application queue
    /// backpressure. Unrelated traffic never acknowledges our outstanding Ping.
    pub(super) fn receive(&self, frame: &Frame) -> Result<bool, ()> {
        if frame.header.class != Class::Result
            || frame.header.family != family::CORE
            || frame.header.kind != yas_wire::core::request_kind::PING
        {
            return Ok(false);
        }
        if frame.header.request_id != Some(REQUEST_ID) {
            return Err(());
        }
        let result = ResultPrefix::decode(&frame.payload).map_err(|_| ())?;
        if result.status != Status::Ok {
            return Err(());
        }
        PingResult::decode(&result.body).map_err(|_| ())?;
        if !self.pending.swap(false, Ordering::AcqRel) {
            return Err(());
        }
        self.replied.notify_one();
        Ok(true)
    }

    /// Returns only on failure. The deadline includes queueing/writing the Ping,
    /// so a non-reading peer cannot keep a session alive by blocking its writer.
    pub(super) async fn run(&self, out: &FrameSender, interval: Duration) {
        if interval.is_zero() {
            std::future::pending::<()>().await;
        }
        loop {
            tokio::time::sleep(interval).await;
            self.pending.store(true, Ordering::Release);
            let frame = Frame {
                header: FrameHeader::request(
                    family::CORE,
                    yas_wire::core::request_kind::PING,
                    REQUEST_ID,
                ),
                payload: Ping {
                    sender_monotonic_ns: monotonic_ns(),
                }
                .encode()
                .expect("fixed Ping payload"),
            };
            let exchange = async {
                out.send(frame).await.map_err(|_| ())?;
                self.replied.notified().await;
                Ok::<(), ()>(())
            };
            if !matches!(
                tokio::time::timeout(interval.saturating_mul(2), exchange).await,
                Ok(Ok(()))
            ) {
                return;
            }
        }
    }
}
