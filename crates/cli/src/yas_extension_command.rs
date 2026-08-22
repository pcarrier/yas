//! Native discovery and invocation for live `@name` extension commands.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, mpsc};
use yas_wire::channel::{self as channel_wire, ChannelEndpoint, Connect};
use yas_wire::extension::{self as extension_wire, CommandPage, DiscoverCommands};
use yas_wire::transfer::{
    Close as TransferClose, Credit, MessageData, MessageReceiver, Reset as TransferReset,
};
use yas_wire::{Class, Decode, Encode, Extensions, Frame, FrameHeader, family};

use crate::yas_native::{NativeClient, NativeFrameReader, NativeFrameSender};

const STDIN_CHUNK: usize = 64 * 1024;
const RECEIVE_WINDOW: u64 = 4 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(31);

const COMMAND_CLIENT_INVOKE: u8 = 1;
const COMMAND_CLIENT_STDIN: u8 = 2;
const COMMAND_CLIENT_STDIN_EOF: u8 = 3;
const COMMAND_CLIENT_CANCEL: u8 = 4;
const INVOKE_FLAG_STDIN: u8 = 1;

const COMMAND_SERVER_STDOUT: u8 = 1;
const COMMAND_SERVER_STDERR: u8 = 2;
const COMMAND_SERVER_LOG: u8 = 3;
const COMMAND_SERVER_RESULT: u8 = 4;
const COMMAND_SERVER_EXIT: u8 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryRecord {
    name: String,
    listener_handle: u64,
    listener_generation: u64,
    descriptor: Descriptor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Descriptor {
    summary: String,
    commands: Vec<DescriptorCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescriptorCommand {
    path: Vec<String>,
    summary: Option<String>,
    usage: Option<String>,
    options: Vec<DescriptorOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DescriptorOption {
    names: Vec<String>,
    takes_value: bool,
    help: Option<String>,
}

pub(super) fn parse_external(tokens: Vec<String>) -> Result<(String, Vec<String>), String> {
    let Some(namespace) = tokens.first() else {
        return Err("missing extension command namespace".into());
    };
    let Some(name) = namespace.strip_prefix('@') else {
        return Err(format!(
            "unknown command `{namespace}` (extension commands use @name)"
        ));
    };
    if !safe_namespace(name) {
        return Err(
            "extension command namespace must be @ followed by 1..=255 safe UTF-8 bytes".into(),
        );
    }
    let name = name.to_owned();
    let args = tokens.into_iter().skip(1).collect::<Vec<_>>();
    validate_invocation_args(&args)?;
    Ok((name, args))
}

pub(super) async fn list(client: &mut NativeClient) -> Result<i32, String> {
    for record in discover(client).await? {
        println!("@{}\t{}", record.name, sanitize(&record.descriptor.summary));
    }
    Ok(0)
}

pub(super) async fn complete(
    client: &mut NativeClient,
    words: &[String],
    current: &str,
) -> Result<Vec<String>, String> {
    let records = discover(client).await?;
    Ok(completion_candidates(&records, words, current))
}

pub(super) async fn invoke(
    client: NativeClient,
    name: &str,
    args: Vec<String>,
    json: bool,
) -> Result<i32, String> {
    let streams_stdin = !std::io::stdin().is_terminal();
    invoke_with_io(
        client,
        name,
        args,
        json,
        InvocationIo {
            streams_stdin,
            stdin: tokio::io::stdin(),
            stdout: tokio::io::stdout(),
            stderr: tokio::io::stderr(),
            cancellation: async {
                tokio::signal::ctrl_c()
                    .await
                    .map_err(|error| format!("cannot listen for Ctrl-C: {error}"))
            },
        },
    )
    .await
}

struct InvocationIo<R, O, E, C> {
    streams_stdin: bool,
    stdin: R,
    stdout: O,
    stderr: E,
    cancellation: C,
}

async fn invoke_with_io<R, O, E, C>(
    mut client: NativeClient,
    name: &str,
    args: Vec<String>,
    json: bool,
    io: InvocationIo<R, O, E, C>,
) -> Result<i32, String>
where
    R: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
    E: AsyncWrite + Unpin,
    C: Future<Output = Result<(), String>>,
{
    validate_invocation_args(&args)?;
    let record = discover(&mut client)
        .await?
        .into_iter()
        .find(|record| record.name == name)
        .ok_or_else(|| format!("extension command namespace not found: @{name}"))?;
    let mut io = io;
    if let Some(help) = local_help(&record, &args) {
        io.stdout
            .write_all(help.as_bytes())
            .await
            .map_err(|error| format!("cannot write command help: {error}"))?;
        io.stdout
            .flush()
            .await
            .map_err(|error| format!("cannot flush command help: {error}"))?;
        return Ok(0);
    }

    let invoke_payload = encode_invoke(&args, io.streams_stdin)?;
    let endpoint: ChannelEndpoint = client
        .request_typed_with_timeout(
            family::CHANNEL,
            channel_wire::request_kind::CONNECT,
            &Connect {
                listener_handle: record.listener_handle,
                generation: record.listener_generation,
                initial_receive_credit: RECEIVE_WINDOW,
                metadata: Vec::new(),
                extensions: Extensions::default(),
            },
            true,
            CONNECT_TIMEOUT,
        )
        .await?;
    let mut channel = CommandChannel::new(client, endpoint)?;
    channel.send_message(&invoke_payload).await?;
    run_channel(&mut channel, json, io).await
}

async fn discover(client: &mut NativeClient) -> Result<Vec<DirectoryRecord>, String> {
    require_command_families(client)?;
    let mut directory_revision = 0u64;
    let mut cursor = 0u64;
    let mut expected_revision = None;
    let mut seen_cursors = HashSet::from([0u64]);
    let mut records = BTreeMap::new();
    loop {
        let page: CommandPage = client
            .request_typed(
                family::EXTENSION,
                extension_wire::request_kind::DISCOVER_COMMANDS,
                &DiscoverCommands {
                    directory_revision,
                    cursor,
                    max_records: 0,
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        if let Some(expected) = expected_revision {
            if page.directory_revision != expected {
                return Err("Extension command directory revision changed mid-snapshot".into());
            }
        } else {
            expected_revision = Some(page.directory_revision);
        }
        for record in page.records {
            let name = record.name;
            let owned = DirectoryRecord {
                descriptor: parse_descriptor(&record.descriptor).map_err(|error| {
                    format!("server advertised an invalid descriptor for @{name}: {error}")
                })?,
                listener_handle: record.listener_handle,
                listener_generation: record.listener_generation,
                name: name.clone(),
            };
            if records.insert(name.clone(), owned).is_some() {
                return Err(format!(
                    "server advertised duplicate extension command namespace @{name}"
                ));
            }
        }
        if page.next_cursor == 0 {
            return Ok(records.into_values().collect());
        }
        if !seen_cursors.insert(page.next_cursor) {
            return Err("server repeated an Extension command directory cursor".into());
        }
        directory_revision = page.directory_revision;
        cursor = page.next_cursor;
    }
}

fn require_command_families(client: &NativeClient) -> Result<(), String> {
    if !client.supports(
        family::EXTENSION,
        Class::Request,
        extension_wire::request_kind::DISCOVER_COMMANDS,
    ) || !client.supports(
        family::CHANNEL,
        Class::Request,
        channel_wire::request_kind::CONNECT,
    ) {
        return Err(
            "server does not provide native Extension discovery and Channel invocation".into(),
        );
    }
    for kind in [
        yas_wire::transfer::kind::MESSAGE_DATA,
        yas_wire::transfer::kind::CREDIT,
        yas_wire::transfer::kind::CLOSE,
        yas_wire::transfer::kind::RESET,
    ] {
        if !client.supports(family::TRANSFER, Class::Event, kind) {
            return Err("server does not provide native MESSAGE Transfers".into());
        }
    }
    Ok(())
}

struct CommandChannel {
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

impl CommandChannel {
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
            return Err("YAS returned an invalid native command Channel descriptor".into());
        }
        let send_credit = Arc::new(AtomicU64::new(descriptor.receiver_send_credit));
        let sent = Arc::new(AtomicU64::new(0));
        let credit_notify = Arc::new(Notify::new());
        let dead = Arc::new(Mutex::new(None));
        let consumed = Arc::new(AtomicU64::new(0));
        let consume_notify = Arc::new(Notify::new());
        let (reader, sender) = client.into_framed();
        let (incoming_tx, incoming) = mpsc::channel(4);
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

    async fn send_message(&mut self, message: &[u8]) -> Result<(), String> {
        if message.is_empty() || message.len() as u64 > self.descriptor.max_item_bytes {
            return Err("extension command Channel message is empty or oversized".into());
        }
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| "extension command Channel message sequence exhausted".to_string())?;
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
                return Err("extension command Channel made no send progress".into());
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

    fn can_send_message(&self, length: usize) -> Result<bool, String> {
        if let Some(error) = self.dead.lock().expect("Channel dead lock").clone() {
            return Err(error);
        }
        let sent = self.sent.load(Ordering::Acquire);
        let end = sent
            .checked_add(length as u64)
            .ok_or_else(|| "extension command Channel send counter overflow".to_string())?;
        Ok(end <= self.send_credit.load(Ordering::Acquire))
    }

    async fn recv(&mut self) -> Result<Vec<u8>, String> {
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
                .unwrap_or_else(|| "native command Channel closed".into())),
        }
    }

    async fn close(&self) {
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

    async fn cancel(&self) {
        let reset = TransferReset {
            transfer_id: self.descriptor.transfer_id,
            status: yas_wire::core::Status::Cancelled.code(),
            detail: Vec::new(),
        };
        let mut header = FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::RESET);
        header.sensitive = true;
        let _ = self
            .sender
            .send(Frame {
                header,
                payload: reset.encode().unwrap_or_default(),
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
    let detail = result.err().unwrap_or_else(|| {
        "extension command Channel closed before the invocation completed".into()
    });
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
    let mut pending = std::collections::VecDeque::<(Vec<u8>, u64)>::new();
    let mut next_sequence = 0u64;
    let mut received = 0u64;
    let mut granted = descriptor.sender_send_credit;
    loop {
        let desired = consumed
            .load(Ordering::Acquire)
            .saturating_add(RECEIVE_WINDOW);
        if desired > granted {
            let credit = Credit {
                transfer_id: descriptor.transfer_id,
                cumulative_limit: desired,
            };
            sender
                .send(Frame {
                    header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT),
                    payload: credit.encode().map_err(wire_error)?,
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
                    return Err("command output consumer stopped".into());
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
                    let permit = permit.map_err(|_| "command output consumer stopped".to_string())?;
                    let (data, wire_bytes) = pending.pop_front().expect("pending command output");
                    permit.send(Incoming::Message { data, wire_bytes });
                    continue;
                }
                _ = consume_notify.notified() => continue,
                frame = reader.next() => frame?,
            }
        };
        if frame.header.family != family::TRANSFER || frame.payload.len() < 4 {
            if frame.header.class == Class::Result {
                return Err("YAS returned an unsolicited Result on a command Channel".into());
            }
            continue;
        }
        let transfer_id = u32::from_le_bytes(frame.payload[..4].try_into().unwrap());
        if transfer_id != descriptor.transfer_id {
            return Err("YAS interleaved an unrelated Transfer on a command Channel".into());
        }
        let sensitive = descriptor
            .requires_sensitive_frame(frame.header.kind)
            .map_err(wire_error)?;
        if frame.header.sensitive != sensitive {
            return Err("command Channel Transfer sensitivity mismatch".into());
        }
        match frame.header.kind {
            yas_wire::transfer::kind::CREDIT => {
                let credit = Credit::decode(&frame.payload).map_err(wire_error)?;
                let previous = send_credit.load(Ordering::Acquire);
                if credit.cumulative_limit < previous
                    || credit.cumulative_limit < sent.load(Ordering::Acquire)
                {
                    return Err("command Channel credit moved backwards".into());
                }
                send_credit.store(credit.cumulative_limit, Ordering::Release);
                credit_notify.notify_one();
            }
            yas_wire::transfer::kind::MESSAGE_DATA => {
                let fragment = MessageData::decode(&frame.payload).map_err(wire_error)?;
                let end = received
                    .checked_add(fragment.data.len() as u64)
                    .ok_or_else(|| "command Channel receive counter overflow".to_string())?;
                if end > granted {
                    return Err("command Channel exceeded receive credit".into());
                }
                let complete = validator.accept(&fragment).map_err(wire_error)?;
                if fragment.start {
                    open.insert(fragment.sequence, Vec::new());
                }
                open.get_mut(&fragment.sequence)
                    .ok_or_else(|| "command Channel lost an open message".to_string())?
                    .extend_from_slice(&fragment.data);
                received = end;
                if complete {
                    let message = open
                        .remove(&fragment.sequence)
                        .ok_or_else(|| "command Channel message disappeared".to_string())?;
                    if completed.insert(fragment.sequence, message).is_some() {
                        return Err("command Channel repeated a message sequence".into());
                    }
                    if completed.len() + open.len() > maximum_buffered_messages {
                        return Err("command Channel exceeded its negotiated message buffer".into());
                    }
                    while let Some(message) = completed.remove(&next_sequence) {
                        let wire_bytes = message.len() as u64;
                        pending.push_back((message, wire_bytes));
                        next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
                            "command Channel receive sequence exhausted".to_string()
                        })?;
                    }
                }
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload).map_err(wire_error)?;
                if close.final_data_bytes != received || !open.is_empty() || !completed.is_empty() {
                    return Err("command Channel closed with incomplete output".into());
                }
                while let Some((data, wire_bytes)) = pending.pop_front() {
                    incoming
                        .send(Incoming::Message { data, wire_bytes })
                        .await
                        .map_err(|_| "command output consumer stopped".to_string())?;
                }
                return Err(format!(
                    "extension command Channel closed before EXIT (status {}): {}",
                    close.status,
                    String::from_utf8_lossy(&close.detail)
                ));
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                return Err(format!(
                    "extension command Channel reset (status {}): {}",
                    reset.status,
                    String::from_utf8_lossy(&reset.detail)
                ));
            }
            other => {
                return Err(format!(
                    "unexpected command Channel Transfer event {other:#06x}"
                ));
            }
        }
    }
}

async fn run_channel<R, O, E, C>(
    channel: &mut CommandChannel,
    json: bool,
    io: InvocationIo<R, O, E, C>,
) -> Result<i32, String>
where
    R: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
    E: AsyncWrite + Unpin,
    C: Future<Output = Result<(), String>>,
{
    let InvocationIo {
        streams_stdin,
        mut stdin,
        mut stdout,
        mut stderr,
        cancellation,
    } = io;
    tokio::pin!(cancellation);
    let mut input = vec![0; STDIN_CHUNK];
    let mut stdin_finished = !streams_stdin;
    let mut pending_input = None::<Vec<u8>>;
    let mut result_seen = false;
    let send_change = Arc::clone(&channel.credit_notify);
    loop {
        if let Some(message) = pending_input.as_ref()
            && channel.can_send_message(message.len())?
        {
            let message = pending_input.take().expect("checked pending command input");
            channel.send_message(&message).await?;
        }
        tokio::select! {
            biased;
            output = channel.recv() => {
                let payload = output?;
                let output = decode_output(&payload, &mut result_seen)?;
                if let Some(code) = deliver_output(output, json, &mut stdout, &mut stderr).await? {
                    stdout.flush().await.map_err(|error| format!("cannot flush command stdout: {error}"))?;
                    stderr.flush().await.map_err(|error| format!("cannot flush command stderr: {error}"))?;
                    channel.close().await;
                    return Ok(code);
                }
            }
            cancelled = &mut cancellation => {
                if channel.can_send_message(1).unwrap_or(false) {
                    let _ = channel.send_message(&[COMMAND_CLIENT_CANCEL]).await;
                    channel.close().await;
                } else {
                    channel.cancel().await;
                }
                return match cancelled {
                    Ok(()) => Ok(130),
                    Err(error) => Err(error),
                };
            }
            _ = send_change.notified(), if pending_input.is_some() => {}
            read = stdin.read(&mut input), if streams_stdin && !stdin_finished && pending_input.is_none() => {
                match read {
                    Ok(0) => {
                        stdin_finished = true;
                        pending_input = Some(vec![COMMAND_CLIENT_STDIN_EOF]);
                    }
                    Ok(count) => {
                        let mut payload = Vec::with_capacity(count + 1);
                        payload.push(COMMAND_CLIENT_STDIN);
                        payload.extend_from_slice(&input[..count]);
                        pending_input = Some(payload);
                    }
                    Err(error) => {
                        channel.cancel().await;
                        return Err(format!("cannot read command stdin: {error}"));
                    }
                }
            }
        }
    }
}

fn completion_candidates(
    records: &[DirectoryRecord],
    words: &[String],
    current: &str,
) -> Vec<String> {
    let Some(command_words) = completion_command_words(words) else {
        return Vec::new();
    };
    let mut candidates = BTreeSet::new();
    let Some((namespace, arguments)) = command_words else {
        for record in records {
            let candidate = format!("@{}", record.name);
            if safe_namespace(&record.name) && candidate.starts_with(current) {
                candidates.insert(candidate);
            }
        }
        return candidates.into_iter().collect();
    };
    let Some(record) = records.iter().find(|record| record.name == namespace) else {
        return Vec::new();
    };
    command_completion_candidates(record, arguments, current, &mut candidates);
    candidates.into_iter().collect()
}

fn completion_command_words(words: &[String]) -> Option<Option<(&str, &[String])>> {
    let mut index = 0usize;
    while index < words.len() {
        match words[index].as_str() {
            "--json" => index += 1,
            "--on" | "--hub" => index = index.checked_add(2)?,
            word if word.starts_with("--on=") || word.starts_with("--hub=") => index += 1,
            word => {
                let namespace = word.strip_prefix('@')?;
                return Some(Some((namespace, &words[index + 1..])));
            }
        }
    }
    Some(None)
}

fn command_completion_candidates(
    record: &DirectoryRecord,
    arguments: &[String],
    current: &str,
    candidates: &mut BTreeSet<String>,
) {
    let commands = record
        .descriptor
        .commands
        .iter()
        .filter(|command| command.path.iter().all(|part| safe_path_part(part)))
        .collect::<Vec<_>>();
    let mut path = Vec::<&str>::new();
    let mut path_open = true;
    let mut options_allowed = true;
    let mut used_options = HashSet::<&str>::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if options_allowed && argument == "--" {
            options_allowed = false;
            path_open = false;
        } else if options_allowed && argument.starts_with('-') {
            path_open = false;
            let (name, inline) = argument
                .split_once('=')
                .map_or((argument, false), |(name, _)| (name, true));
            if let Some(option) = exact_command(&commands, &path).and_then(|command| {
                command
                    .options
                    .iter()
                    .find(|option| option.names.iter().any(|candidate| candidate == name))
            }) {
                used_options.extend(option.names.iter().map(String::as_str));
                if option.takes_value && !inline {
                    index += 1;
                }
            }
        } else if path_open
            && commands.iter().any(|command| {
                command.path.len() > path.len()
                    && command.path[..path.len()]
                        .iter()
                        .map(String::as_str)
                        .eq(path.iter().copied())
                    && command.path[path.len()] == argument
            })
        {
            path.push(argument);
        } else {
            path_open = false;
        }
        index += 1;
    }
    if path_open {
        for command in &commands {
            if command.path.len() > path.len()
                && command.path[..path.len()]
                    .iter()
                    .map(String::as_str)
                    .eq(path.iter().copied())
            {
                let candidate = &command.path[path.len()];
                if candidate.starts_with(current) {
                    candidates.insert(candidate.clone());
                }
            }
        }
    }
    if options_allowed && let Some(command) = exact_command(&commands, &path) {
        for option in &command.options {
            if option
                .names
                .iter()
                .any(|name| used_options.contains(name.as_str()))
            {
                continue;
            }
            for name in &option.names {
                if safe_option_name(name) && name.starts_with(current) {
                    candidates.insert(name.clone());
                }
            }
        }
    }
}

fn exact_command<'a>(
    commands: &[&'a DescriptorCommand],
    path: &[&str],
) -> Option<&'a DescriptorCommand> {
    commands.iter().copied().find(|command| {
        command
            .path
            .iter()
            .map(String::as_str)
            .eq(path.iter().copied())
    })
}

fn parse_descriptor(source: &str) -> Result<Descriptor, String> {
    let value: Value =
        serde_json::from_str(source).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "descriptor root is not an object".to_string())?;
    if object.get("protocol").and_then(Value::as_str) != Some("yas.cli.v1") {
        return Err("unsupported command protocol".into());
    }
    let summary = object
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(|| "descriptor summary is missing".to_string())?
        .to_owned();
    let commands = object
        .get("commands")
        .and_then(Value::as_array)
        .ok_or_else(|| "descriptor commands array is missing".to_string())?
        .iter()
        .filter_map(parse_descriptor_command)
        .collect();
    Ok(Descriptor { summary, commands })
}

fn parse_descriptor_command(value: &Value) -> Option<DescriptorCommand> {
    let object = value.as_object()?;
    let path = object
        .get("path")?
        .as_array()?
        .iter()
        .map(|part| part.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let options = object
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_descriptor_option)
        .collect();
    Some(DescriptorCommand {
        path,
        summary: object
            .get("summary")
            .and_then(Value::as_str)
            .map(str::to_owned),
        usage: object
            .get("usage")
            .and_then(Value::as_str)
            .map(str::to_owned),
        options,
    })
}

fn parse_descriptor_option(value: &Value) -> Option<DescriptorOption> {
    let object = value.as_object()?;
    let names = object
        .get("names")?
        .as_array()?
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    if names.is_empty() {
        return None;
    }
    Some(DescriptorOption {
        names,
        takes_value: object
            .get("takes_value")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        help: object
            .get("help")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn local_help(record: &DirectoryRecord, args: &[String]) -> Option<String> {
    if args.last().map(String::as_str) != Some("--help") {
        return None;
    }
    let path = &args[..args.len() - 1];
    if path.is_empty() {
        return Some(render_root_help(record));
    }
    let command = record
        .descriptor
        .commands
        .iter()
        .find(|command| command.path == path)?;
    Some(render_command_help(record, command))
}

fn render_root_help(record: &DirectoryRecord) -> String {
    let mut output = format!(
        "@{} — {}\n",
        record.name,
        sanitize(&record.descriptor.summary)
    );
    if let Some(root) = record
        .descriptor
        .commands
        .iter()
        .find(|command| command.path.is_empty())
    {
        append_usage_options(&mut output, &record.name, root);
    }
    let commands = record
        .descriptor
        .commands
        .iter()
        .filter(|command| !command.path.is_empty())
        .collect::<Vec<_>>();
    if !commands.is_empty() {
        output.push_str("\nCommands:\n");
        for command in commands {
            output.push_str("  ");
            output.push_str(&command.path.join(" "));
            if let Some(summary) = &command.summary {
                output.push('\t');
                output.push_str(&sanitize(summary));
            }
            output.push('\n');
        }
    }
    output
}

fn render_command_help(record: &DirectoryRecord, command: &DescriptorCommand) -> String {
    let mut output = format!("@{} {}", record.name, command.path.join(" "));
    if let Some(summary) = &command.summary {
        output.push_str(" — ");
        output.push_str(&sanitize(summary));
    }
    output.push('\n');
    append_usage_options(&mut output, &record.name, command);
    output
}

fn append_usage_options(output: &mut String, name: &str, command: &DescriptorCommand) {
    if let Some(usage) = &command.usage {
        output.push_str("Usage: @");
        output.push_str(name);
        output.push(' ');
        output.push_str(&sanitize(usage));
        output.push('\n');
    }
    if !command.options.is_empty() {
        output.push_str("\nOptions:\n");
        for option in &command.options {
            output.push_str("  ");
            output.push_str(&option.names.join(", "));
            if option.takes_value {
                output.push_str(" <VALUE>");
            }
            if let Some(help) = &option.help {
                output.push('\t');
                output.push_str(&sanitize(help));
            }
            output.push('\n');
        }
    }
}

fn validate_invocation_args(args: &[String]) -> Result<(), String> {
    if args.len() > extension_wire::MAX_ARGS {
        return Err(format!(
            "too many extension command arguments (maximum {})",
            extension_wire::MAX_ARGS
        ));
    }
    let mut argument_bytes = 0usize;
    let mut encoded_bytes = 4usize;
    for argument in args {
        if argument.len() > extension_wire::MAX_ARG_BYTES {
            return Err(format!(
                "extension command argument exceeds {} bytes",
                extension_wire::MAX_ARG_BYTES
            ));
        }
        argument_bytes = argument_bytes
            .checked_add(argument.len())
            .ok_or_else(|| "extension command arguments are too large".to_string())?;
        encoded_bytes = encoded_bytes
            .checked_add(4 + argument.len())
            .ok_or_else(|| "extension command invocation is too large".to_string())?;
    }
    if argument_bytes > extension_wire::MAX_ARGUMENT_BYTES {
        return Err(format!(
            "extension command arguments exceed {} bytes",
            extension_wire::MAX_ARGUMENT_BYTES
        ));
    }
    if encoded_bytes as u64 > channel_wire::MAX_MESSAGE_BYTES {
        return Err(format!(
            "encoded extension command invocation exceeds {} bytes",
            channel_wire::MAX_MESSAGE_BYTES
        ));
    }
    Ok(())
}

fn encode_invoke(args: &[String], streams_stdin: bool) -> Result<Vec<u8>, String> {
    validate_invocation_args(args)?;
    let mut payload = Vec::with_capacity(
        4 + args
            .iter()
            .map(|argument| 4 + argument.len())
            .sum::<usize>(),
    );
    payload.push(COMMAND_CLIENT_INVOKE);
    payload.push(u8::from(streams_stdin) * INVOKE_FLAG_STDIN);
    payload.extend_from_slice(&(args.len() as u16).to_le_bytes());
    for argument in args {
        payload.extend_from_slice(&(argument.len() as u32).to_le_bytes());
        payload.extend_from_slice(argument.as_bytes());
    }
    Ok(payload)
}

enum CommandOutput<'a> {
    Stdout(&'a [u8]),
    Stderr(&'a [u8]),
    Log {
        level: u8,
        message: &'a str,
    },
    Result {
        content_type: &'a str,
        data: &'a [u8],
    },
    Exit {
        code: i32,
        detail: &'a str,
    },
}

fn decode_output<'a>(
    payload: &'a [u8],
    result_seen: &mut bool,
) -> Result<CommandOutput<'a>, String> {
    let Some((&kind, body)) = payload.split_first() else {
        return Err("extension command sent an empty message".into());
    };
    match kind {
        COMMAND_SERVER_STDOUT => Ok(CommandOutput::Stdout(body)),
        COMMAND_SERVER_STDERR => Ok(CommandOutput::Stderr(body)),
        COMMAND_SERVER_LOG => {
            let Some((&level, message)) = body.split_first() else {
                return Err("extension command LOG is truncated".into());
            };
            if level > 4 {
                return Err("extension command LOG level is invalid".into());
            }
            Ok(CommandOutput::Log {
                level,
                message: std::str::from_utf8(message)
                    .map_err(|_| "extension command LOG is not UTF-8".to_string())?,
            })
        }
        COMMAND_SERVER_RESULT => {
            if *result_seen || body.len() < 2 {
                return Err("extension command RESULT is duplicate or truncated".into());
            }
            let length = u16::from_le_bytes([body[0], body[1]]) as usize;
            let end = 2usize
                .checked_add(length)
                .ok_or_else(|| "extension command RESULT length overflow".to_string())?;
            let content_type = std::str::from_utf8(
                body.get(2..end)
                    .ok_or_else(|| "extension command RESULT type is truncated".to_string())?,
            )
            .map_err(|_| "extension command RESULT type is not UTF-8".to_string())?;
            if !valid_content_type(content_type) {
                return Err("extension command RESULT content type is invalid".into());
            }
            *result_seen = true;
            Ok(CommandOutput::Result {
                content_type,
                data: &body[end..],
            })
        }
        COMMAND_SERVER_EXIT => {
            if body.len() < 4 {
                return Err("extension command EXIT is truncated".into());
            }
            Ok(CommandOutput::Exit {
                code: i32::from_le_bytes(body[..4].try_into().unwrap()),
                detail: std::str::from_utf8(&body[4..])
                    .map_err(|_| "extension command EXIT detail is not UTF-8".to_string())?,
            })
        }
        _ => Err(format!(
            "extension command sent unknown yas.cli.v1 kind {kind}"
        )),
    }
}

async fn deliver_output<O: AsyncWrite + Unpin, E: AsyncWrite + Unpin>(
    output: CommandOutput<'_>,
    json: bool,
    stdout: &mut O,
    stderr: &mut E,
) -> Result<Option<i32>, String> {
    if json {
        let (record, exit) = match output {
            CommandOutput::Stdout(data) => (serde_json::json!({"type":"stdout","data":data}), None),
            CommandOutput::Stderr(data) => (serde_json::json!({"type":"stderr","data":data}), None),
            CommandOutput::Log { level, message } => (
                serde_json::json!({"type":"log","level":level,"message":message}),
                None,
            ),
            CommandOutput::Result { content_type, data } => (
                serde_json::json!({"type":"result","content_type":content_type,"data":data}),
                None,
            ),
            CommandOutput::Exit { code, detail } => (
                serde_json::json!({"type":"exit","code":code,"detail":detail}),
                Some(code),
            ),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|error| format!("cannot encode command JSON: {error}"))?;
        line.push(b'\n');
        stdout
            .write_all(&line)
            .await
            .map_err(|error| format!("cannot write command JSON: {error}"))?;
        return Ok(exit);
    }
    match output {
        CommandOutput::Stdout(data) | CommandOutput::Result { data, .. } => stdout
            .write_all(data)
            .await
            .map_err(|error| format!("cannot write command stdout: {error}"))?,
        CommandOutput::Stderr(data) => stderr
            .write_all(data)
            .await
            .map_err(|error| format!("cannot write command stderr: {error}"))?,
        CommandOutput::Log { level, message } => {
            let prefix =
                ["[trace] ", "[debug] ", "[info] ", "[warning] ", "[error] "][level as usize];
            stderr
                .write_all(prefix.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            stderr
                .write_all(message.as_bytes())
                .await
                .map_err(|error| error.to_string())?;
            if !message.ends_with('\n') {
                stderr
                    .write_all(b"\n")
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        CommandOutput::Exit { code, detail } => {
            if !detail.is_empty() {
                stderr
                    .write_all(detail.as_bytes())
                    .await
                    .map_err(|error| error.to_string())?;
                if !detail.ends_with('\n') {
                    stderr
                        .write_all(b"\n")
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            return Ok(Some(code));
        }
    }
    Ok(None)
}

fn valid_content_type(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    let mut parts = value.split('/');
    let (Some(left), Some(right), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    valid_media_component(left) && valid_media_component(right)
}

fn valid_media_component(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&^_.+-".contains(&byte)
        })
}

fn safe_namespace(value: &str) -> bool {
    safe_bare_token(value, extension_wire::MAX_NAME_BYTES)
}

fn safe_path_part(value: &str) -> bool {
    safe_bare_token(value, 255)
}

fn safe_bare_token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn safe_option_name(value: &str) -> bool {
    let Some(body) = value.strip_prefix('-') else {
        return false;
    };
    let body = body.strip_prefix('-').unwrap_or(body);
    !body.is_empty()
        && value.len() <= 255
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn wire_error(error: impl std::fmt::Display) -> String {
    format!("invalid native YAS command value: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTOR: &str = r#"{
      "protocol":"yas.cli.v1",
      "summary":"Build and publish",
      "commands":[{"path":["build"],"usage":"build TARGET","options":[{"names":["-r","--release"],"takes_value":false}]}]
    }"#;

    fn record() -> DirectoryRecord {
        DirectoryRecord {
            name: "builder".into(),
            listener_handle: 11,
            listener_generation: 12,
            descriptor: parse_descriptor(DESCRIPTOR).unwrap(),
        }
    }

    #[test]
    fn external_namespace_is_explicit_and_verbatim() {
        assert_eq!(
            parse_external(vec!["@builder".into(), "--remote-option".into()]).unwrap(),
            ("builder".into(), vec!["--remote-option".into()])
        );
        assert!(parse_external(vec!["builder".into()]).is_err());
    }

    #[test]
    fn descriptor_drives_help_and_static_completion() {
        let record = record();
        assert!(
            local_help(&record, &["build".into(), "--help".into()])
                .unwrap()
                .contains("Usage: @builder build TARGET")
        );
        assert_eq!(
            completion_candidates(&[record], &["@builder".into(), "build".into()], "--r"),
            vec!["--release"]
        );
    }

    #[test]
    fn invocation_and_output_codecs_are_bounded() {
        let payload = encode_invoke(&["build".into()], true).unwrap();
        assert_eq!(
            &payload[..4],
            &[COMMAND_CLIENT_INVOKE, INVOKE_FLAG_STDIN, 1, 0]
        );
        let mut seen = false;
        let mut exit = vec![COMMAND_SERVER_EXIT];
        exit.extend_from_slice(&7i32.to_le_bytes());
        exit.extend_from_slice(b"done");
        assert!(matches!(
            decode_output(&exit, &mut seen).unwrap(),
            CommandOutput::Exit {
                code: 7,
                detail: "done"
            }
        ));
    }
}
