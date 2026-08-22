use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Notify, mpsc};
use yas_wire::channel::{self as channel_wire, ChannelEndpoint, Connect, ListenerRecord};
use yas_wire::transfer::{
    Close as TransferClose, Credit, MessageData, MessageReceiver, Reset as TransferReset,
};
use yas_wire::{Class, Decode, Encode, Extensions, Frame, FrameHeader, family};

use crate::yas_native::{NativeClient, NativeFrameReader, NativeFrameSender};

const CHANNEL_NAME: &str = "yas.muster.v1";
const RECEIVE_WINDOW: u64 = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(31);

pub(super) async fn connect(on: Option<&str>, hub: &str) -> Result<MessageChannel, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let listener = client
        .snapshot(family::CHANNEL)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Channel family".to_string())?
        .into_iter()
        .filter_map(|record| ListenerRecord::from_state_record(&record).ok())
        .find(|record| record.name == CHANNEL_NAME)
        .ok_or_else(|| {
            format!("Channel {CHANNEL_NAME} has no listener; is the muster extension running?")
        })?;
    let endpoint: ChannelEndpoint = client
        .request_typed_with_timeout(
            family::CHANNEL,
            channel_wire::request_kind::CONNECT,
            &Connect {
                listener_handle: listener.listener_handle,
                generation: listener.generation,
                initial_receive_credit: RECEIVE_WINDOW,
                metadata: Vec::new(),
                extensions: Extensions::default(),
            },
            true,
            CONNECT_TIMEOUT,
        )
        .await?;
    MessageChannel::new(client, endpoint)
}

pub(super) struct MessageChannel {
    sender: NativeFrameSender,
    descriptor: yas_wire::transfer::Descriptor,
    send_credit: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
    credit_notify: Arc<Notify>,
    dead: Arc<Mutex<Option<String>>>,
    consumed: Arc<AtomicU64>,
    consume_notify: Arc<Notify>,
    incoming: mpsc::Receiver<Incoming>,
    next_sequence: u64,
}

enum Incoming {
    Message { data: Vec<u8>, wire_bytes: u64 },
    Closed(String),
}

impl MessageChannel {
    fn new(client: NativeClient, endpoint: ChannelEndpoint) -> Result<Self, String> {
        let descriptor = endpoint.descriptor;
        descriptor.validate().map_err(wire_error)?;
        if descriptor.mode != yas_wire::transfer::Mode::Message
            || descriptor.direction != yas_wire::transfer::Direction::BIDIRECTIONAL
            || descriptor.content_family != family::CHANNEL
            || descriptor.content_kind != channel_wire::CHANNEL_CONTENT_KIND
            || descriptor.content_version != channel_wire::VERSION
            || !descriptor.sensitive_content().map_err(wire_error)?
        {
            return Err("YAS returned an invalid native Muster Channel descriptor".into());
        }
        let send_credit = Arc::new(AtomicU64::new(descriptor.receiver_send_credit));
        let sent = Arc::new(AtomicU64::new(0));
        let credit_notify = Arc::new(Notify::new());
        let dead = Arc::new(Mutex::new(None));
        let consumed = Arc::new(AtomicU64::new(0));
        let consume_notify = Arc::new(Notify::new());
        let (reader, sender) = client.into_framed();
        let (incoming_tx, incoming) = mpsc::channel(16);
        tokio::spawn(read_channel(
            reader,
            ChannelReadState {
                sender: sender.clone(),
                descriptor: descriptor.clone(),
                send_credit: Arc::clone(&send_credit),
                sent: Arc::clone(&sent),
                credit_notify: Arc::clone(&credit_notify),
                dead: Arc::clone(&dead),
                consumed: Arc::clone(&consumed),
                consume_notify: Arc::clone(&consume_notify),
                incoming: incoming_tx,
            },
        ));
        Ok(Self {
            sender,
            descriptor,
            send_credit,
            sent,
            credit_notify,
            dead,
            consumed,
            consume_notify,
            incoming,
            next_sequence: 0,
        })
    }

    pub(super) async fn send(&mut self, message: &str) -> Result<(), String> {
        let message = message.as_bytes();
        if message.is_empty() || message.len() as u64 > self.descriptor.max_item_bytes {
            return Err("Muster Channel message is empty or oversized".into());
        }
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "Muster Channel message sequence exhausted".to_string())?;
        let mut offset = 0usize;
        while offset < message.len() {
            let available = loop {
                if let Some(error) = self.dead.lock().expect("Channel dead lock").clone() {
                    return Err(error);
                }
                let sent = self.sent.load(Ordering::Acquire);
                let credit = self.send_credit.load(Ordering::Acquire);
                if credit > sent {
                    break usize::try_from(credit - sent).unwrap_or(usize::MAX);
                }
                self.credit_notify.notified().await;
            };
            let length = (message.len() - offset)
                .min(self.descriptor.max_chunk_bytes as usize)
                .min(available);
            if length == 0 {
                return Err("Muster Channel made no send progress".into());
            }
            let end = offset + length;
            let fragment = MessageData {
                transfer_id: self.descriptor.transfer_id,
                sequence,
                fragment_offset: offset as u64,
                start: offset == 0,
                end: end == message.len(),
                data: message[offset..end].to_vec(),
            };
            let mut header =
                FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::MESSAGE_DATA);
            header.sensitive = true;
            self.sender
                .send(Frame {
                    header,
                    payload: fragment.encode().map_err(wire_error)?,
                })
                .await?;
            self.sent.fetch_add(length as u64, Ordering::Release);
            offset = end;
        }
        Ok(())
    }

    pub(super) async fn recv(&mut self) -> Result<Vec<u8>, String> {
        match self.incoming.recv().await {
            Some(Incoming::Message { data, wire_bytes }) => {
                self.consumed.fetch_add(wire_bytes, Ordering::Release);
                self.consume_notify.notify_one();
                Ok(data)
            }
            Some(Incoming::Closed(detail)) => Err(detail),
            None => Err(self
                .dead
                .lock()
                .expect("Channel dead lock")
                .clone()
                .unwrap_or_else(|| "native Muster Channel closed".into())),
        }
    }

    pub(super) async fn close(&self) {
        let sent = self.sent.load(Ordering::Acquire);
        let close = TransferClose {
            transfer_id: self.descriptor.transfer_id,
            final_data_bytes: sent,
            status: yas_wire::core::Status::Ok.code(),
            detail: Vec::new(),
        };
        let mut header = FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE);
        header.sensitive = true;
        let _ = self
            .sender
            .send(Frame {
                header,
                payload: close.encode().unwrap_or_default(),
            })
            .await;
    }
}

struct ChannelReadState {
    sender: NativeFrameSender,
    descriptor: yas_wire::transfer::Descriptor,
    send_credit: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
    credit_notify: Arc<Notify>,
    dead: Arc<Mutex<Option<String>>>,
    consumed: Arc<AtomicU64>,
    consume_notify: Arc<Notify>,
    incoming: mpsc::Sender<Incoming>,
}

async fn read_channel(mut reader: NativeFrameReader, state: ChannelReadState) {
    let result = read_channel_inner(&mut reader, &state).await;
    let detail = result
        .err()
        .unwrap_or_else(|| "native Muster Channel closed".into());
    *state.dead.lock().expect("Channel dead lock") = Some(detail.clone());
    state.credit_notify.notify_one();
    let _ = state.incoming.send(Incoming::Closed(detail)).await;
}

async fn read_channel_inner(
    reader: &mut NativeFrameReader,
    state: &ChannelReadState,
) -> Result<(), String> {
    let ChannelReadState {
        sender,
        descriptor,
        send_credit,
        sent,
        credit_notify,
        consumed,
        consume_notify,
        incoming,
        ..
    } = state;
    let mut validator = MessageReceiver::new(descriptor).map_err(wire_error)?;
    let maximum_buffered_messages = descriptor.max_open_messages().map_err(wire_error)? as usize;
    let mut open = BTreeMap::<u64, Vec<u8>>::new();
    let mut completed = BTreeMap::<u64, Vec<u8>>::new();
    let mut pending = VecDeque::<(Vec<u8>, u64)>::new();
    let mut next_sequence = 0u64;
    let mut received = 0u64;
    let mut granted = descriptor.sender_send_credit;
    loop {
        let desired = consumed
            .load(Ordering::Acquire)
            .saturating_add(RECEIVE_WINDOW);
        if desired > granted {
            sender
                .send(Frame {
                    header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT),
                    payload: Credit {
                        transfer_id: descriptor.transfer_id,
                        cumulative_limit: desired,
                    }
                    .encode()
                    .map_err(wire_error)?,
                })
                .await?;
            granted = desired;
        }
        while let Some((data, wire_bytes)) = pending.pop_front() {
            match incoming.try_send(Incoming::Message { data, wire_bytes }) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(Incoming::Message { data, wire_bytes })) => {
                    pending.push_front((data, wire_bytes));
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    return Err("Muster Channel consumer stopped".into());
                }
                Err(mpsc::error::TrySendError::Full(Incoming::Closed(_))) => unreachable!(),
            }
        }
        let frame = if pending.is_empty() {
            tokio::select! {
                _ = consume_notify.notified() => continue,
                frame = reader.next() => frame?,
            }
        } else {
            tokio::select! {
                permit = incoming.reserve() => {
                    let permit = permit.map_err(|_| "Muster Channel consumer stopped".to_string())?;
                    let (data, wire_bytes) = pending.pop_front().expect("pending Muster message");
                    permit.send(Incoming::Message { data, wire_bytes });
                    continue;
                }
                _ = consume_notify.notified() => continue,
                frame = reader.next() => frame?,
            }
        };
        if frame.header.family != family::TRANSFER || frame.payload.len() < 4 {
            if frame.header.class == Class::Result {
                return Err("YAS returned an unsolicited Result on the Muster Channel".into());
            }
            continue;
        }
        let transfer_id = u32::from_le_bytes(frame.payload[..4].try_into().unwrap());
        if transfer_id != descriptor.transfer_id {
            return Err("YAS interleaved an unrelated Transfer on the Muster Channel".into());
        }
        let sensitive = descriptor
            .requires_sensitive_frame(frame.header.kind)
            .map_err(wire_error)?;
        if frame.header.sensitive != sensitive {
            return Err("Muster Channel Transfer sensitivity mismatch".into());
        }
        match frame.header.kind {
            yas_wire::transfer::kind::CREDIT => {
                let credit = Credit::decode(&frame.payload).map_err(wire_error)?;
                let previous = send_credit.load(Ordering::Acquire);
                if credit.cumulative_limit < previous
                    || credit.cumulative_limit < sent.load(Ordering::Acquire)
                {
                    return Err("Muster Channel credit moved backwards".into());
                }
                send_credit.store(credit.cumulative_limit, Ordering::Release);
                credit_notify.notify_one();
            }
            yas_wire::transfer::kind::MESSAGE_DATA => {
                let fragment = MessageData::decode(&frame.payload).map_err(wire_error)?;
                let end = received
                    .checked_add(fragment.data.len() as u64)
                    .ok_or_else(|| "Muster Channel receive counter overflow".to_string())?;
                if end > granted {
                    return Err("Muster Channel exceeded receive credit".into());
                }
                let complete = validator.accept(&fragment).map_err(wire_error)?;
                if fragment.start {
                    open.insert(fragment.sequence, Vec::new());
                }
                open.get_mut(&fragment.sequence)
                    .ok_or_else(|| "Muster Channel lost an open message".to_string())?
                    .extend_from_slice(&fragment.data);
                received = end;
                if complete {
                    let message = open
                        .remove(&fragment.sequence)
                        .ok_or_else(|| "Muster Channel message disappeared".to_string())?;
                    if completed.insert(fragment.sequence, message).is_some() {
                        return Err("Muster Channel repeated a message sequence".into());
                    }
                    if completed.len() + open.len() > maximum_buffered_messages {
                        return Err("Muster Channel exceeded its negotiated message buffer".into());
                    }
                    while let Some(message) = completed.remove(&next_sequence) {
                        let wire_bytes = message.len() as u64;
                        pending.push_back((message, wire_bytes));
                        next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
                            "Muster Channel receive sequence exhausted".to_string()
                        })?;
                    }
                }
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload).map_err(wire_error)?;
                if close.final_data_bytes != received || !open.is_empty() || !completed.is_empty() {
                    return Err("Muster Channel closed with incomplete messages".into());
                }
                while let Some((data, wire_bytes)) = pending.pop_front() {
                    incoming
                        .send(Incoming::Message { data, wire_bytes })
                        .await
                        .map_err(|_| "Muster Channel consumer stopped".to_string())?;
                }
                return Err(format!(
                    "Muster Channel closed (status {}): {}",
                    close.status,
                    String::from_utf8_lossy(&close.detail)
                ));
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                return Err(format!(
                    "Muster Channel reset (status {}): {}",
                    reset.status,
                    String::from_utf8_lossy(&reset.detail)
                ));
            }
            other => {
                return Err(format!(
                    "unexpected Muster Channel Transfer event {other:#06x}"
                ));
            }
        }
    }
}

fn wire_error(error: yas_wire::Error) -> String {
    format!("YAS protocol: {error}")
}
