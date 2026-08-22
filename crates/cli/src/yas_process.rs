//! Native YAS Process implementation for `yas run`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use clap::Args;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use yas_wire::{Decode, Extensions, Frame, core::Status, family, process, transfer};

use crate::yas_native::NativeClient;

const STREAM_RECEIVE_WINDOW: u64 = 1024 * 1024;

#[derive(Args, Clone, Debug)]
pub struct RunArgs {
    /// Working directory for the process
    #[arg(long = "in", value_name = "DIR")]
    pub directory: Option<OsString>,

    /// Set an environment variable, repeatable (--env KEY=VALUE)
    #[arg(long, value_name = "KEY=VALUE")]
    pub env: Vec<OsString>,

    /// Program to execute directly
    #[arg(value_name = "PROGRAM", allow_hyphen_values = true)]
    pub program: OsString,

    /// Arguments passed verbatim to the program
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub arguments: Vec<OsString>,
}

pub(crate) async fn run(on: Option<&str>, hub: &str, args: RunArgs) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    run_with_stdio(
        &mut client,
        args,
        tokio::io::stdin(),
        tokio::io::stdout(),
        tokio::io::stderr(),
    )
    .await
}

async fn run_with_stdio<R, O, E>(
    client: &mut NativeClient,
    args: RunArgs,
    mut stdin: R,
    mut stdout: O,
    mut stderr: E,
) -> Result<i32, String>
where
    R: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
    E: AsyncWrite + Unpin,
{
    let spawn = build_spawn(args)?;
    let bundle: process::StreamBundle = client
        .request_typed(family::PROCESS, process::request_kind::SPAWN, &spawn, true)
        .await?;

    let mut stdout_state = OutputState::new(bundle.stdout)?;
    let mut stderr_state = bundle.stderr.map(OutputState::new).transpose()?;
    let mut stdin_state = bundle.stdin.map(InputState::new).transpose()?;
    let stdin_chunk = stdin_state
        .as_ref()
        .map(|state| state.descriptor.max_chunk_bytes as usize)
        .unwrap_or(1)
        .min(64 * 1024);
    let mut stdin_buffer = vec![0; stdin_chunk.max(1)];

    while !stdout_state.closed || stderr_state.as_ref().is_some_and(|stream| !stream.closed) {
        let stdin_read_len = stdin_state
            .as_ref()
            .filter(|state| !state.closed)
            .and_then(|state| state.credit.checked_sub(state.sent))
            .and_then(|available| usize::try_from(available).ok())
            .unwrap_or(0)
            .min(stdin_buffer.len());

        tokio::select! {
            read = stdin.read(&mut stdin_buffer[..stdin_read_len]), if stdin_read_len != 0 => {
                let count = read.map_err(|error| format!("cannot read stdin: {error}"))?;
                let state = stdin_state.as_mut().expect("guarded stdin state");
                if count == 0 {
                    client.send_typed_event(
                        family::TRANSFER,
                        transfer::kind::CLOSE,
                        &transfer::Close {
                            transfer_id: state.descriptor.transfer_id,
                            final_data_bytes: state.sent,
                            status: Status::Ok.code(),
                            detail: Vec::new(),
                        },
                        true,
                    ).await?;
                    state.closed = true;
                } else {
                    let data = transfer::ByteData {
                        transfer_id: state.descriptor.transfer_id,
                        offset: state.sent,
                        data: stdin_buffer[..count].to_vec(),
                    };
                    client.send_typed_event(
                        family::TRANSFER,
                        transfer::kind::BYTE_DATA,
                        &data,
                        true,
                    ).await?;
                    state.sent = state.sent.checked_add(count as u64)
                        .ok_or_else(|| "Process stdin offset overflow".to_string())?;
                }
            }
            frame = client.next_event() => {
                let frame = frame?;
                handle_transfer_event(
                    client,
                    frame,
                    stdin_state.as_mut(),
                    &mut stdout_state,
                    stderr_state.as_mut(),
                    &mut stdout,
                    &mut stderr,
                ).await?;
            }
        }
    }

    stdout
        .flush()
        .await
        .map_err(|error| format!("cannot flush stdout: {error}"))?;
    stderr
        .flush()
        .await
        .map_err(|error| format!("cannot flush stderr: {error}"))?;

    let exit: process::ExitRecord = client
        .request_typed(
            family::PROCESS,
            process::request_kind::WAIT,
            &process::Wait {
                process_handle: bundle.process_handle,
                timeout_ns: 0,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    exit_code(exit)
}

async fn handle_transfer_event<O, E>(
    client: &mut NativeClient,
    frame: Frame,
    stdin: Option<&mut InputState>,
    stdout: &mut OutputState,
    stderr: Option<&mut OutputState>,
    stdout_writer: &mut O,
    stderr_writer: &mut E,
) -> Result<(), String>
where
    O: AsyncWrite + Unpin,
    E: AsyncWrite + Unpin,
{
    if frame.header.family != family::TRANSFER {
        return Ok(());
    }
    match frame.header.kind {
        transfer::kind::CREDIT => {
            if frame.header.sensitive {
                return Err("Process Transfer CREDIT was marked sensitive".into());
            }
            let credit = transfer::Credit::decode(&frame.payload)
                .map_err(|error| format!("invalid Process Transfer CREDIT: {error}"))?;
            let Some(stdin) =
                stdin.filter(|stream| stream.descriptor.transfer_id == credit.transfer_id)
            else {
                return Err(format!(
                    "Process received CREDIT for unknown Transfer {:#010x}",
                    credit.transfer_id
                ));
            };
            if credit.cumulative_limit <= stdin.credit {
                return Err("Process stdin credit did not increase".into());
            }
            stdin.credit = credit.cumulative_limit;
        }
        transfer::kind::BYTE_DATA => {
            let data = transfer::ByteData::decode(&frame.payload)
                .map_err(|error| format!("invalid Process BYTE_DATA: {error}"))?;
            if data.transfer_id == stdout.descriptor.transfer_id {
                receive_output_chunk(client, &frame, stdout, stdout_writer, data).await?;
            } else if let Some(stderr) =
                stderr.filter(|stream| stream.descriptor.transfer_id == data.transfer_id)
            {
                receive_output_chunk(client, &frame, stderr, stderr_writer, data).await?;
            } else {
                return Err(format!(
                    "Process received bytes for unknown Transfer {:#010x}",
                    data.transfer_id
                ));
            }
        }
        transfer::kind::CLOSE => {
            let close = transfer::Close::decode(&frame.payload)
                .map_err(|error| format!("invalid Process Transfer CLOSE: {error}"))?;
            if close.transfer_id == stdout.descriptor.transfer_id {
                close_output(&frame, stdout, &close)?;
            } else if let Some(stderr) =
                stderr.filter(|stream| stream.descriptor.transfer_id == close.transfer_id)
            {
                close_output(&frame, stderr, &close)?;
            } else {
                return Err(format!(
                    "Process received CLOSE for unknown Transfer {:#010x}",
                    close.transfer_id
                ));
            }
        }
        transfer::kind::RESET => {
            let reset = transfer::Reset::decode(&frame.payload)
                .map_err(|error| format!("invalid Process Transfer RESET: {error}"))?;
            let descriptor = if reset.transfer_id == stdout.descriptor.transfer_id {
                Some(&stdout.descriptor)
            } else if let Some(stderr) = stderr
                .as_deref()
                .filter(|stream| stream.descriptor.transfer_id == reset.transfer_id)
            {
                Some(&stderr.descriptor)
            } else {
                stdin
                    .as_deref()
                    .filter(|stream| stream.descriptor.transfer_id == reset.transfer_id)
                    .map(|stream| &stream.descriptor)
            }
            .ok_or_else(|| {
                format!(
                    "Process received RESET for unknown Transfer {:#010x}",
                    reset.transfer_id
                )
            })?;
            require_sensitivity(descriptor, &frame)?;
            return Err(format!(
                "Process Transfer {:#010x} reset with status {}: {}",
                reset.transfer_id,
                reset.status,
                String::from_utf8_lossy(&reset.detail)
            ));
        }
        other => return Err(format!("unexpected Process Transfer event {other:#06x}")),
    }
    Ok(())
}

async fn receive_output_chunk<W: AsyncWrite + Unpin>(
    client: &mut NativeClient,
    frame: &Frame,
    stream: &mut OutputState,
    writer: &mut W,
    data: transfer::ByteData,
) -> Result<(), String> {
    require_sensitivity(&stream.descriptor, frame)?;
    if stream.closed
        || data.offset != stream.received
        || data.data.len() > stream.descriptor.max_chunk_bytes as usize
    {
        return Err(format!(
            "Process Transfer {:#010x} sent a non-contiguous, oversized, or post-CLOSE chunk",
            data.transfer_id
        ));
    }
    writer
        .write_all(&data.data)
        .await
        .map_err(|error| format!("cannot write process output: {error}"))?;
    stream.received = stream
        .received
        .checked_add(data.data.len() as u64)
        .ok_or_else(|| "Process output offset overflow".to_string())?;
    let cumulative_limit = stream
        .received
        .checked_add(stream.window)
        .ok_or_else(|| "Process output credit overflow".to_string())?;
    client
        .send_typed_event(
            family::TRANSFER,
            transfer::kind::CREDIT,
            &transfer::Credit {
                transfer_id: stream.descriptor.transfer_id,
                cumulative_limit,
            },
            false,
        )
        .await
}

fn close_output(
    frame: &Frame,
    stream: &mut OutputState,
    close: &transfer::Close,
) -> Result<(), String> {
    require_sensitivity(&stream.descriptor, frame)?;
    if stream.closed || close.final_data_bytes != stream.received {
        return Err(format!(
            "Process Transfer {:#010x} CLOSE length mismatch",
            close.transfer_id
        ));
    }
    if close.status != Status::Ok.code() {
        return Err(format!(
            "Process Transfer {:#010x} closed with status {}: {}",
            close.transfer_id,
            close.status,
            String::from_utf8_lossy(&close.detail)
        ));
    }
    stream.closed = true;
    Ok(())
}

fn require_sensitivity(descriptor: &transfer::Descriptor, frame: &Frame) -> Result<(), String> {
    let required = descriptor
        .requires_sensitive_frame(frame.header.kind)
        .map_err(|error| format!("invalid Process Transfer descriptor: {error}"))?;
    if frame.header.sensitive != required {
        return Err(format!(
            "Process Transfer {:#010x} sensitivity flag mismatch",
            descriptor.transfer_id
        ));
    }
    Ok(())
}

struct InputState {
    descriptor: transfer::Descriptor,
    sent: u64,
    credit: u64,
    closed: bool,
}

impl InputState {
    fn new(descriptor: transfer::Descriptor) -> Result<Self, String> {
        if descriptor.receiver_send_credit == 0 {
            return Err("Process stdin Transfer has zero initial credit".into());
        }
        let credit = descriptor.receiver_send_credit;
        Ok(Self {
            descriptor,
            sent: 0,
            credit,
            closed: false,
        })
    }
}

struct OutputState {
    descriptor: transfer::Descriptor,
    received: u64,
    window: u64,
    closed: bool,
}

impl OutputState {
    fn new(descriptor: transfer::Descriptor) -> Result<Self, String> {
        if descriptor.sender_send_credit == 0 {
            return Err("Process output Transfer has zero initial credit".into());
        }
        let window = descriptor.sender_send_credit;
        Ok(Self {
            descriptor,
            received: 0,
            window,
            closed: false,
        })
    }
}

fn build_spawn(args: RunArgs) -> Result<process::Spawn, String> {
    let RunArgs {
        directory,
        env,
        program,
        arguments,
    } = args;
    let mut argv = Vec::with_capacity(arguments.len() + 1);
    argv.push(os_bytes(&program)?);
    for argument in arguments {
        argv.push(os_bytes(&argument)?);
    }

    let mut environment = BTreeMap::<Vec<u8>, Vec<u8>>::new();
    for assignment in env {
        let bytes = os_bytes(&assignment)?;
        let (key, value) = split_env_assignment(&bytes, &assignment)?;
        environment.insert(key.to_vec(), value.to_vec());
    }

    Ok(process::Spawn {
        operation_id: operation_id(),
        flags: 0,
        environment_kind: process::EnvironmentKind::Session,
        cwd: match directory {
            Some(directory) => process::Cwd::Path(os_bytes(&directory)?),
            None => process::Cwd::ServerDefault,
        },
        argv,
        env: environment
            .into_iter()
            .map(|(key, value)| process::EnvEntry { key, value })
            .collect(),
        stdout_receive_credit: STREAM_RECEIVE_WINDOW,
        stderr_receive_credit: STREAM_RECEIVE_WINDOW,
        extensions: Extensions::default(),
    })
}

fn exit_code(exit: process::ExitRecord) -> Result<i32, String> {
    match exit.kind {
        process::ExitKind::Code => Ok(exit.code),
        process::ExitKind::Signal => Ok(128i32.saturating_add(exit.code).clamp(0, 255)),
        process::ExitKind::Killed | process::ExitKind::Other => Err(if exit.detail.is_empty() {
            format!(
                "process ended abnormally ({:?}, reason {})",
                exit.kind, exit.reason
            )
        } else {
            String::from_utf8_lossy(&exit.detail).into_owned()
        }),
    }
}

fn split_env_assignment<'a>(
    bytes: &'a [u8],
    original: &OsStr,
) -> Result<(&'a [u8], &'a [u8]), String> {
    match bytes.iter().position(|byte| *byte == b'=') {
        Some(0) => Err(format!("--env needs a name before the '=': {original:?}")),
        Some(index) => Ok((&bytes[..index], &bytes[index + 1..])),
        None => Err(format!("--env needs KEY=VALUE, got {original:?}")),
    }
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Result<Vec<u8>, String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(value.as_bytes().to_vec())
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Result<Vec<u8>, String> {
    value
        .to_str()
        .map(|value| value.as_bytes().to_vec())
        .ok_or_else(|| format!("process arguments must be valid UTF-8 on Windows: {value:?}"))
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(value: &OsStr) -> Result<Vec<u8>, String> {
    Ok(value.as_encoded_bytes().to_vec())
}

fn operation_id() -> [u8; 16] {
    let mut value: [u8; 16] = rand::random();
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::Encode;

    #[test]
    fn spawn_inherits_session_and_sorts_environment() {
        let spawn = build_spawn(RunArgs {
            directory: Some(OsString::from("/work")),
            env: vec![OsString::from("Z=1"), OsString::from("A=2")],
            program: OsString::from("echo"),
            arguments: vec![OsString::from("hello")],
        })
        .unwrap();
        assert_eq!(spawn.environment_kind, process::EnvironmentKind::Session);
        assert_eq!(spawn.cwd, process::Cwd::Path(b"/work".to_vec()));
        assert_eq!(spawn.argv, [b"echo".to_vec(), b"hello".to_vec()]);
        assert_eq!(
            spawn
                .env
                .iter()
                .map(|entry| entry.key.as_slice())
                .collect::<Vec<_>>(),
            [b"A".as_slice(), b"Z".as_slice()]
        );
        spawn.encode().unwrap();
    }

    #[test]
    fn numeric_and_signal_exit_codes_are_preserved() {
        let mut exit = process::ExitRecord {
            kind: process::ExitKind::Code,
            reason: 0,
            code: 17,
            exited_server_ns: 1,
            detail: Vec::new(),
        };
        assert_eq!(exit_code(exit.clone()).unwrap(), 17);
        exit.kind = process::ExitKind::Signal;
        exit.reason = yas_wire::schema::process::EXIT_REASON_INTERRUPT as u8;
        exit.code = 2;
        assert_eq!(exit_code(exit).unwrap(), 130);
    }
}
