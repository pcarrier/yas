//! Native YAS Extension client.
//!
//! Lifecycle, object admission, output following, command discovery, and
//! command invocation all use the native Extension/Channel/Transfer families.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};
use yas_wire::extension::{
    self as wire, Control, ControlAction, DefinitionIdentity, Deploy, ExtensionRecord, Follow,
    FollowResult, ObjectBegin, ObjectBeginResult, ObjectCommit, ObjectDisposition, OutputBatch,
    OutputKind, Phase, Runtime, RuntimeLimits,
};
use yas_wire::schema::extension as schema;
use yas_wire::transfer::{
    Close as TransferClose, Credit, MessageData, MessageReceiver, Reset as TransferReset,
};
use yas_wire::{Class, Decode, Encode, Extensions, family};

use crate::yas_native::NativeClient;

#[path = "yas_extension_command.rs"]
mod command_cli;

#[path = "yas_extension_manage.rs"]
mod manage;

const FOLLOW_WINDOW: u64 = 8 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

impl fmt::Display for RestartPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Never => "never",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        })
    }
}

#[derive(Args, Clone, Debug)]
pub(crate) struct RunArgs {
    /// Return once the extension has reached RUNNING
    #[arg(long)]
    pub detach: bool,

    /// Store an enabled, desired-running definition (implies --detach)
    #[arg(long)]
    pub persist: bool,

    /// Attempt restart policy
    #[arg(long, value_enum, default_value_t)]
    pub restart: RestartPolicy,

    /// Emit NDJSON lifecycle and output records
    #[arg(long)]
    pub json: bool,

    /// A label, or the unique durable name under --persist
    pub name: String,

    /// Wasm or JavaScript extension (a path or an https:// URL), then UTF-8 arguments
    #[arg(
        value_names = ["MODULE", "ARGS"],
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub invocation: Vec<OsString>,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct UpdateArgs {
    /// Replace the stored restart policy (preserved when omitted)
    #[arg(long, value_enum)]
    pub restart: Option<RestartPolicy>,

    /// Emit an NDJSON lifecycle record
    #[arg(long)]
    pub json: bool,

    /// Exact persistent extension name
    pub name: String,

    /// Replacement Wasm or JavaScript extension, then UTF-8 arguments
    #[arg(
        value_names = ["MODULE", "ARGS"],
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub invocation: Vec<OsString>,
}

#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ExtensionCommand {
    /// Choose extensions to install, update, or uninstall
    Manage(manage::ManageArgs),

    /// Execute a Wasm or JavaScript extension
    Run(RunArgs),

    /// List visible extensions
    #[command(alias = "ls")]
    List {
        /// Emit one NDJSON record per extension
        #[arg(long)]
        json: bool,
    },

    /// Show one extension's current lifecycle snapshot
    Status {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Follow retained and future output from an extension
    Attach {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Start a stopped extension
    ///
    /// Marks it desired-running and begins an attempt. Already running is not
    /// an error: this is the verb for "I want this up", and it is idempotent.
    Start {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Stop a running extension
    ///
    /// Clears desired-running and cancels the current attempt. Durable enable
    /// state is untouched — see `disable` for the answer that survives a
    /// restart of the server.
    #[command(alias = "cancel")]
    Stop {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Replace a persistent extension definition
    Update(UpdateArgs),

    /// Start a fresh attempt immediately
    Restart {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Durably enable a persistent extension
    Enable {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Durably disable a persistent extension
    Disable {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// Remove a disabled, quiescent persistent extension
    Remove {
        selector: String,
        #[arg(long)]
        json: bool,
    },

    /// List live extension-provided command namespaces
    Commands,
}

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: ExtensionCommand,
) -> Result<i32, String> {
    if let ExtensionCommand::Manage(args) = command {
        return manage::run(on, hub, args).await;
    }
    let mut client = NativeClient::connect(on, hub).await?;
    require_lifecycle(&client)?;
    match command {
        ExtensionCommand::Manage(_) => unreachable!(),
        ExtensionCommand::Run(args) => run(&mut client, args).await,
        ExtensionCommand::List { json } => {
            print_list(&snapshot(&mut client).await?, json);
            Ok(0)
        }
        ExtensionCommand::Status { selector, json } => {
            let record = resolve_selector(&mut client, &selector).await?;
            render_record(&record, json, "status");
            Ok(0)
        }
        ExtensionCommand::Attach { selector, json } => {
            let record = resolve_selector(&mut client, &selector).await?;
            follow_until_terminal(&mut client, record, json, false).await
        }
        ExtensionCommand::Start { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Start, json).await
        }
        ExtensionCommand::Stop { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Stop, json).await
        }
        ExtensionCommand::Update(args) => update(&mut client, args).await,
        ExtensionCommand::Restart { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Restart, json).await
        }
        ExtensionCommand::Enable { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Enable, json).await
        }
        ExtensionCommand::Disable { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Disable, json).await
        }
        ExtensionCommand::Remove { selector, json } => {
            control_once(&mut client, &selector, ControlAction::Remove, json).await
        }
        ExtensionCommand::Commands => command_cli::list(&mut client).await,
    }
}

pub(crate) fn parse_advertised_command(
    tokens: Vec<String>,
) -> Result<(String, Vec<String>), String> {
    command_cli::parse_external(tokens)
}

pub(crate) async fn dispatch_advertised_command(
    on: Option<&str>,
    hub: &str,
    name: String,
    args: Vec<String>,
    json: bool,
) -> Result<i32, String> {
    let client = NativeClient::connect(on, hub).await?;
    command_cli::invoke(client, &name, args, json).await
}

pub(crate) async fn complete_advertised_commands(
    on: Option<&str>,
    hub: &str,
    words: &[String],
    current: &str,
) -> Result<Vec<String>, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    command_cli::complete(&mut client, words, current).await
}

fn require_lifecycle(client: &NativeClient) -> Result<(), String> {
    for kind in [
        wire::request_kind::WATCH,
        wire::request_kind::UNWATCH,
        wire::request_kind::OBJECT_BEGIN,
        wire::request_kind::OBJECT_COMMIT,
        wire::request_kind::DEPLOY,
        wire::request_kind::CONTROL,
        wire::request_kind::FOLLOW,
    ] {
        if !client.supports(family::EXTENSION, Class::Request, kind) {
            return Err("YAS server does not provide the complete native Extension family".into());
        }
    }
    for kind in [
        yas_wire::transfer::kind::BYTE_DATA,
        yas_wire::transfer::kind::MESSAGE_DATA,
        yas_wire::transfer::kind::CREDIT,
        yas_wire::transfer::kind::CLOSE,
        yas_wire::transfer::kind::RESET,
    ] {
        if !client.supports(family::TRANSFER, Class::Event, kind) {
            return Err("YAS server does not provide native Transfer streams".into());
        }
    }
    Ok(())
}

async fn run(client: &mut NativeClient, args: RunArgs) -> Result<i32, String> {
    validate_name(&args.name)?;
    let (source, argv) = split_invocation(args.invocation)?;
    validate_args(&argv)?;
    let object = source.load().await?;
    admit_object(client, &object).await?;

    let detached = args.detach || args.persist;
    let mut flags = (schema::DEFINITION_ENABLED | schema::DEFINITION_DESIRED_RUNNING) as u16;
    if detached {
        flags |= schema::DEFINITION_DETACHED as u16;
    }
    if args.persist {
        flags |= schema::DEFINITION_PERSISTENT as u16;
    }
    let identity = deploy(
        client,
        Deploy {
            operation_id: operation_id(),
            expected_extension_handle: 0,
            expected_generation: 0,
            expected_definition_revision: 0,
            flags,
            runtime: Runtime::Auto,
            restart_policy: restart_policy(args.restart),
            name: args.name,
            content_hash: object.hash,
            argv: argv.into_iter().map(String::into_bytes).collect(),
            runtime_limits: default_runtime_limits(),
            extensions: Extensions::default(),
        },
    )
    .await?;
    let record = wait_for_identity(client, &identity).await?;
    if args.json {
        render_record(&record, true, "status");
    }
    if detached {
        wait_until_started(client, identity, record, args.json).await
    } else {
        follow_until_terminal(client, record, args.json, true).await
    }
}

async fn update(client: &mut NativeClient, args: UpdateArgs) -> Result<i32, String> {
    let name = forced_name(&args.name)?.to_owned();
    validate_name(&name)?;
    let current = find_named(&snapshot(client).await?, &name)?.clone();
    let (source, argv) = split_invocation(args.invocation)?;
    validate_args(&argv)?;
    let object = source.load().await?;
    admit_object(client, &object).await?;
    let restart = args
        .restart
        .map(restart_policy)
        .unwrap_or(current.restart_policy);
    let identity = deploy(
        client,
        Deploy {
            operation_id: operation_id(),
            expected_extension_handle: current.extension_handle,
            expected_generation: current.generation,
            expected_definition_revision: current.definition_revision,
            flags: current.flags,
            runtime: Runtime::Auto,
            restart_policy: restart,
            name,
            content_hash: object.hash,
            argv: argv.into_iter().map(String::into_bytes).collect(),
            runtime_limits: current.runtime_limits,
            extensions: Extensions::default(),
        },
    )
    .await?;
    let record = wait_for_identity(client, &identity).await?;
    render_record(&record, args.json, "status");
    Ok(0)
}

async fn admit_object(client: &mut NativeClient, object: &ModuleObject) -> Result<(), String> {
    let begin: ObjectBeginResult = client
        .request_typed(
            family::EXTENSION,
            wire::request_kind::OBJECT_BEGIN,
            &ObjectBegin {
                operation_id: operation_id(),
                content_hash: object.hash,
                byte_len: object.bytes.len() as u64,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    if begin.disposition == ObjectDisposition::AlreadyPresent {
        return Ok(());
    }
    let descriptor = begin
        .descriptor
        .as_ref()
        .ok_or_else(|| "YAS requested an Extension upload without a Transfer".to_string())?;
    client.send_byte_transfer(descriptor, &object.bytes).await?;
    let _: Vec<u8> = client
        .request(
            family::EXTENSION,
            wire::request_kind::OBJECT_COMMIT,
            ObjectCommit {
                staging_handle: begin.staging_handle,
                operation_id: operation_id(),
                content_hash: object.hash,
                byte_len: object.bytes.len() as u64,
                extensions: Extensions::default(),
            }
            .encode()
            .map_err(wire_error)?,
            true,
        )
        .await?;
    Ok(())
}

async fn deploy(client: &mut NativeClient, request: Deploy) -> Result<DefinitionIdentity, String> {
    client
        .request_typed(
            family::EXTENSION,
            wire::request_kind::DEPLOY,
            &request,
            true,
        )
        .await
}

async fn control_once(
    client: &mut NativeClient,
    selector_text: &str,
    action: ControlAction,
    json: bool,
) -> Result<i32, String> {
    let current = resolve_selector(client, selector_text).await?;
    let identity = control(client, &current, action).await?;
    if action == ControlAction::Remove {
        render_identity(&identity, json, action_name(action));
    } else {
        let record = wait_for_identity(client, &identity).await?;
        render_record(&record, json, action_name(action));
    }
    Ok(0)
}

async fn control(
    client: &mut NativeClient,
    current: &ExtensionRecord,
    action: ControlAction,
) -> Result<DefinitionIdentity, String> {
    client
        .request_typed(
            family::EXTENSION,
            wire::request_kind::CONTROL,
            &Control {
                extension_handle: current.extension_handle,
                generation: current.generation,
                expected_definition_revision: current.definition_revision,
                operation_id: operation_id(),
                action,
                extensions: Extensions::default(),
            },
            true,
        )
        .await
}

async fn wait_until_started(
    client: &mut NativeClient,
    identity: DefinitionIdentity,
    mut current: ExtensionRecord,
    json: bool,
) -> Result<i32, String> {
    // This is a newly-created definition. DEPLOY has no baseline field; for a
    // creation the baseline is definitionally zero, even if the first short
    // attempt completed before the following snapshot arrived.
    let baseline = 0;
    loop {
        if current.phase == Phase::Running || current.last_running_attempt > baseline {
            if !json {
                render_record(&current, false, "status");
            }
            return Ok(0);
        }
        if matches!(current.phase, Phase::Stopped | Phase::Blocked) {
            render_record(&current, json, "status");
            return Ok(exit_code(&current).max(1));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        current = wait_for_identity(client, &identity).await?;
    }
}

async fn follow_until_terminal(
    client: &mut NativeClient,
    mut current: ExtensionRecord,
    json: bool,
    stop_on_interrupt: bool,
) -> Result<i32, String> {
    let mut followed_attempt = 0u64;
    loop {
        let attempt = current.attempt.max(current.last_running_attempt);
        if attempt > followed_attempt {
            match follow_once(client, &current, attempt, json).await? {
                FollowOutcome::Completed => followed_attempt = attempt,
                FollowOutcome::Interrupted => {
                    if stop_on_interrupt {
                        let _: DefinitionIdentity = client
                            .request_typed(
                                family::EXTENSION,
                                wire::request_kind::CONTROL,
                                &Control {
                                    extension_handle: current.extension_handle,
                                    generation: current.generation,
                                    expected_definition_revision: 0,
                                    operation_id: operation_id(),
                                    action: ControlAction::Stop,
                                    extensions: Extensions::default(),
                                },
                                true,
                            )
                            .await?;
                    }
                    return Ok(130);
                }
            }
        }
        current = find_identity(
            &snapshot(client).await?,
            current.extension_handle,
            current.generation,
        )?
        .clone();
        if matches!(current.phase, Phase::Stopped | Phase::Blocked) {
            if json {
                render_record(&current, true, "status");
            }
            return Ok(exit_code(&current));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

enum FollowOutcome {
    Completed,
    Interrupted,
}

async fn follow_once(
    client: &mut NativeClient,
    record: &ExtensionRecord,
    attempt: u64,
    json: bool,
) -> Result<FollowOutcome, String> {
    let result: FollowResult = client
        .request_typed(
            family::EXTENSION,
            wire::request_kind::FOLLOW,
            &Follow {
                extension_handle: record.extension_handle,
                generation: record.generation,
                attempt,
                from_sequence: 0,
                initial_receive_credit: FOLLOW_WINDOW,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let descriptor = &result.descriptor;
    let mut validator = MessageReceiver::new(descriptor).map_err(wire_error)?;
    let mut open = BTreeMap::<u64, Vec<u8>>::new();
    let mut received = 0u64;
    let mut granted = descriptor.sender_send_credit;
    let transfer_id = descriptor.transfer_id;
    let cancellation = tokio::signal::ctrl_c();
    tokio::pin!(cancellation);
    loop {
        let frame = tokio::select! {
            frame = client.next_event() => frame?,
            cancelled = &mut cancellation => {
                cancelled.map_err(|error| format!("cannot listen for Ctrl-C: {error}"))?;
                client.send_typed_event(
                    family::TRANSFER,
                    yas_wire::transfer::kind::RESET,
                    &TransferReset {
                        transfer_id,
                        status: yas_wire::core::Status::Cancelled.code(),
                        detail: Vec::new(),
                    },
                    true,
                ).await?;
                return Ok(FollowOutcome::Interrupted);
            }
        };
        if frame.header.family != family::TRANSFER || frame.payload.len() < 4 {
            continue;
        }
        let event_transfer = u32::from_le_bytes(frame.payload[..4].try_into().unwrap());
        if event_transfer != transfer_id {
            return Err(
                "YAS interleaved an unrelated Transfer while following an extension".into(),
            );
        }
        let sensitive = descriptor
            .requires_sensitive_frame(frame.header.kind)
            .map_err(wire_error)?;
        if frame.header.sensitive != sensitive {
            return Err("Extension output Transfer sensitivity mismatch".into());
        }
        match frame.header.kind {
            yas_wire::transfer::kind::MESSAGE_DATA => {
                let fragment = MessageData::decode(&frame.payload).map_err(wire_error)?;
                let complete = validator.accept(&fragment).map_err(wire_error)?;
                received = received
                    .checked_add(fragment.data.len() as u64)
                    .ok_or_else(|| "Extension output byte counter overflow".to_string())?;
                if fragment.start {
                    open.insert(fragment.sequence, Vec::new());
                }
                let message = open
                    .get_mut(&fragment.sequence)
                    .ok_or_else(|| "Extension output lost an open message".to_string())?;
                message.extend_from_slice(&fragment.data);
                if complete {
                    let message = open
                        .remove(&fragment.sequence)
                        .ok_or_else(|| "Extension output message disappeared".to_string())?;
                    let batch = OutputBatch::decode(&message).map_err(wire_error)?;
                    render_batch(&batch, json)?;
                    let next = received.saturating_add(FOLLOW_WINDOW);
                    if next > granted {
                        client
                            .send_typed_event(
                                family::TRANSFER,
                                yas_wire::transfer::kind::CREDIT,
                                &Credit {
                                    transfer_id,
                                    cumulative_limit: next,
                                },
                                false,
                            )
                            .await?;
                        granted = next;
                    }
                }
            }
            yas_wire::transfer::kind::CLOSE => {
                let close = TransferClose::decode(&frame.payload).map_err(wire_error)?;
                if close.final_data_bytes != received || !open.is_empty() {
                    return Err("Extension output Transfer closed with incomplete data".into());
                }
                if close.status != yas_wire::core::Status::Ok.code() {
                    return Err(format!(
                        "Extension output Transfer closed with status {}: {}",
                        close.status,
                        String::from_utf8_lossy(&close.detail)
                    ));
                }
                std::io::stdout()
                    .flush()
                    .map_err(|error| error.to_string())?;
                std::io::stderr()
                    .flush()
                    .map_err(|error| error.to_string())?;
                return Ok(FollowOutcome::Completed);
            }
            yas_wire::transfer::kind::RESET => {
                let reset = TransferReset::decode(&frame.payload).map_err(wire_error)?;
                return Err(format!(
                    "Extension output Transfer reset with status {}: {}",
                    reset.status,
                    String::from_utf8_lossy(&reset.detail)
                ));
            }
            other => {
                return Err(format!(
                    "Extension output Transfer sent unexpected event {other:#06x}"
                ));
            }
        }
    }
}

fn render_batch(batch: &OutputBatch, json: bool) -> Result<(), String> {
    for record in &batch.records {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "type": match record.kind {
                        OutputKind::Stdout => "stdout",
                        OutputKind::Stderr => "stderr",
                        OutputKind::Log => "log",
                        OutputKind::Gap => "gap",
                    },
                    "sequence": record.sequence,
                    "server_ns": record.server_ns,
                    "data": record.data,
                })
            );
            continue;
        }
        match record.kind {
            OutputKind::Stdout => std::io::stdout()
                .write_all(&record.data)
                .map_err(|error| format!("writing extension stdout: {error}"))?,
            OutputKind::Stderr => std::io::stderr()
                .write_all(&record.data)
                .map_err(|error| format!("writing extension stderr: {error}"))?,
            OutputKind::Log => eprintln!("{}", String::from_utf8_lossy(&record.data)),
            OutputKind::Gap => {
                let lost = u64::from_le_bytes(
                    record
                        .data
                        .as_slice()
                        .try_into()
                        .map_err(|_| "invalid Extension output gap".to_string())?,
                );
                eprintln!("yas: {lost} extension output record(s) were evicted");
            }
        }
    }
    Ok(())
}

async fn snapshot(client: &mut NativeClient) -> Result<Vec<ExtensionRecord>, String> {
    let records = client
        .snapshot(family::EXTENSION)
        .await?
        .ok_or_else(|| "YAS server does not expose native Extension state".to_string())?;
    let mut extensions = records
        .iter()
        .map(ExtensionRecord::from_state_record)
        .collect::<Result<Vec<_>, _>>()
        .map_err(wire_error)?;
    extensions.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.extension_handle.cmp(&right.extension_handle))
    });
    Ok(extensions)
}

async fn wait_for_identity(
    client: &mut NativeClient,
    identity: &DefinitionIdentity,
) -> Result<ExtensionRecord, String> {
    for _ in 0..50 {
        let records = snapshot(client).await?;
        if let Ok(record) = find_identity(&records, identity.extension_handle, identity.generation)
            && record.definition_revision >= identity.definition_revision
        {
            return Ok(record.clone());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err("Extension mutation succeeded but its native state record did not appear".into())
}

enum Selector<'a> {
    Handle(u64),
    Name(&'a str),
}

fn selector(text: &str) -> Result<Selector<'_>, String> {
    if let Some(handle) = text.strip_prefix("id:") {
        if handle.len() != 16 || !handle.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("extension handles use id:<16-hex-digits>".into());
        }
        return u64::from_str_radix(handle, 16)
            .map(Selector::Handle)
            .map_err(|_| "invalid extension handle".into());
    }
    let name = text.strip_prefix("name:").unwrap_or(text);
    if name.is_empty() {
        return Err("extension name cannot be empty".into());
    }
    Ok(Selector::Name(name))
}

fn forced_name(text: &str) -> Result<&str, String> {
    match selector(text)? {
        Selector::Name(name) => Ok(name),
        Selector::Handle(_) => Err("update requires an exact persistent name, not an ID".into()),
    }
}

async fn resolve_selector(
    client: &mut NativeClient,
    text: &str,
) -> Result<ExtensionRecord, String> {
    let records = snapshot(client).await?;
    match selector(text)? {
        Selector::Handle(handle) => records
            .into_iter()
            .find(|record| record.extension_handle == handle)
            .ok_or_else(|| format!("extension handle not found: {handle:016x}")),
        Selector::Name(name) => Ok(find_selectable(&records, name)?.clone()),
    }
}

fn find_identity(
    records: &[ExtensionRecord],
    handle: u64,
    generation: u64,
) -> Result<&ExtensionRecord, String> {
    records
        .iter()
        .find(|record| record.extension_handle == handle && record.generation == generation)
        .ok_or_else(|| format!("extension id:{handle:016x} is no longer present"))
}

fn find_named<'a>(
    records: &'a [ExtensionRecord],
    name: &str,
) -> Result<&'a ExtensionRecord, String> {
    records
        .iter()
        .find(|record| {
            record.name == name && record.flags & schema::DEFINITION_PERSISTENT as u16 != 0
        })
        .ok_or_else(|| format!("extension name not found: {name}"))
}

fn find_selectable<'a>(
    records: &'a [ExtensionRecord],
    name: &str,
) -> Result<&'a ExtensionRecord, String> {
    if let Ok(record) = find_named(records, name) {
        return Ok(record);
    }
    let mut matches = records.iter().filter(|record| record.name == name);
    match (matches.next(), matches.next()) {
        (Some(record), None) => Ok(record),
        (Some(_), Some(_)) => Err(format!(
            "{name} names more than one transient extension; select it by id:"
        )),
        _ => Err(format!("extension name not found: {name}")),
    }
}

fn print_list(records: &[ExtensionRecord], json: bool) {
    for record in records {
        render_record(record, json, "extension");
    }
}

fn render_record(record: &ExtensionRecord, json: bool, kind: &str) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "type": kind,
                "extension_handle": format_id(record.extension_handle),
                "generation": record.generation,
                "definition_revision": record.definition_revision,
                "name": record.name,
                "phase": phase_name(record.phase),
                "runtime": runtime_name(record.runtime),
                "flags": record.flags,
                "persistent": record.flags & schema::DEFINITION_PERSISTENT as u16 != 0,
                "enabled": record.flags & schema::DEFINITION_ENABLED as u16 != 0,
                "desired_running": record.flags & schema::DEFINITION_DESIRED_RUNNING as u16 != 0,
                "detached": record.flags & schema::DEFINITION_DETACHED as u16 != 0,
                "restart": restart_name(record.restart_policy),
                "attempt": record.attempt,
                "last_running_attempt": record.last_running_attempt,
                "task_id": record.task_id,
                "next_start_unix_ms": record.next_start_unix_ms,
                "directory_revision": record.directory_revision,
                "hash": hex(&record.content_hash),
                "last_exit": record.last_exit.as_ref().map(|exit| serde_json::json!({
                    "kind": format!("{:?}", exit.kind).to_ascii_lowercase(),
                    "code": exit.code,
                    "attempt": exit.attempt,
                    "server_ns": exit.server_ns,
                    "detail": exit.detail,
                })),
            })
        );
    } else {
        println!(
            "id:{}\t{}\trevision={}\tphase={}\trestart={}\tattempt={}\thash={}",
            format_id(record.extension_handle),
            record.name,
            record.definition_revision,
            phase_name(record.phase),
            restart_name(record.restart_policy),
            record.attempt,
            hex(&record.content_hash),
        );
    }
}

fn render_identity(identity: &DefinitionIdentity, json: bool, action: &str) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "type": action,
                "extension_handle": format_id(identity.extension_handle),
                "generation": identity.generation,
                "definition_revision": identity.definition_revision,
            })
        );
    } else {
        println!(
            "id:{}\tgeneration={}\trevision={}\t{action}",
            format_id(identity.extension_handle),
            identity.generation,
            identity.definition_revision,
        );
    }
}

fn exit_code(record: &ExtensionRecord) -> i32 {
    match record.last_exit.as_ref() {
        Some(exit) if exit.kind == wire::ExitKind::Returned => exit.code,
        Some(_) => 1,
        None if record.phase == Phase::Blocked => 1,
        None => 0,
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::NeedObject => "need-object",
        Phase::Validating => "validating",
        Phase::Queued => "queued",
        Phase::Running => "running",
        Phase::Backoff => "backoff",
        Phase::Stopped => "stopped",
        Phase::Blocked => "blocked",
        Phase::Stopping => "stopping",
    }
}

fn runtime_name(runtime: Runtime) -> &'static str {
    match runtime {
        Runtime::Auto => "auto",
        Runtime::Wasmi => "wasmi",
        Runtime::QuickJs => "quickjs",
    }
}

fn restart_name(restart: wire::RestartPolicy) -> &'static str {
    match restart {
        wire::RestartPolicy::Never => "never",
        wire::RestartPolicy::OnFailure => "on-failure",
        wire::RestartPolicy::Always => "always",
    }
}

fn action_name(action: ControlAction) -> &'static str {
    match action {
        ControlAction::Stop => "stop",
        ControlAction::Start => "start",
        ControlAction::Restart => "restart",
        ControlAction::Enable => "enable",
        ControlAction::Disable => "disable",
        ControlAction::Remove => "remove",
    }
}

fn restart_policy(value: RestartPolicy) -> wire::RestartPolicy {
    match value {
        RestartPolicy::Never => wire::RestartPolicy::Never,
        RestartPolicy::OnFailure => wire::RestartPolicy::OnFailure,
        RestartPolicy::Always => wire::RestartPolicy::Always,
    }
}

fn default_runtime_limits() -> RuntimeLimits {
    RuntimeLimits {
        memory_bytes: 0,
        stack_bytes: 0,
        max_active_jobs: 0,
        max_pending_jobs: 0,
        max_job_bytes: 0,
        slow_consumer_timeout_ns: 0,
        extensions: Extensions::default(),
    }
}

fn operation_id() -> [u8; 16] {
    loop {
        let id = rand::random();
        if id != [0; 16] {
            return id;
        }
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > wire::MAX_NAME_BYTES
        || name.as_bytes().contains(&0)
        || name.chars().any(char::is_control)
    {
        return Err(
            "extension name must be 1..=255 UTF-8 bytes with no NUL or control characters".into(),
        );
    }
    Ok(())
}

/// Options of `ext run`/`ext update` themselves, which everything after
/// MODULE would otherwise carry off to the extension.
const OWN_OPTIONS: [&str; 4] = ["--restart", "--persist", "--detach", "--json"];

fn split_invocation(invocation: Vec<OsString>) -> Result<(ModuleSource, Vec<String>), String> {
    let mut invocation = invocation.into_iter();
    let source = invocation
        .next()
        .ok_or_else(|| "missing extension object MODULE".to_string())
        .and_then(ModuleSource::parse)?;
    let mut args = invocation
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "extension arguments must be valid UTF-8".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Everything after MODULE belongs to the extension, hyphens and all — so
    // `--restart always` written after it becomes two arguments to the
    // extension rather than a policy, and the command reports success having
    // done neither thing the user asked for. Say so, and take `--` as the
    // answer for an extension that genuinely wants such an argument.
    if args.first().is_some_and(|argument| argument == "--") {
        args.remove(0);
    } else if let Some(misplaced) = args.iter().find(|argument| {
        OWN_OPTIONS
            .iter()
            .any(|option| *argument == option || argument.starts_with(&format!("{option}=")))
    }) {
        return Err(format!(
            "{misplaced} after MODULE is an argument to the extension, not an option: \
             write it before the extension name, or after `--` to mean it"
        ));
    }
    Ok((source, args))
}

fn validate_args(args: &[String]) -> Result<(), String> {
    if args.len() > wire::MAX_ARGS {
        return Err(format!(
            "too many extension arguments (maximum {})",
            wire::MAX_ARGS
        ));
    }
    let mut total = 0usize;
    for argument in args {
        if argument.len() > wire::MAX_ARG_BYTES {
            return Err(format!(
                "extension argument exceeds {} bytes",
                wire::MAX_ARG_BYTES
            ));
        }
        total = total
            .checked_add(argument.len())
            .ok_or_else(|| "extension arguments are too large".to_string())?;
    }
    if total > wire::MAX_ARGUMENT_BYTES {
        return Err(format!(
            "extension arguments exceed {} bytes",
            wire::MAX_ARGUMENT_BYTES
        ));
    }
    Ok(())
}

fn format_id(handle: u64) -> String {
    format!("{handle:016x}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        use fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
        output
    })
}

fn wire_error(error: impl fmt::Display) -> String {
    format!("invalid native YAS Extension value: {error}")
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ModuleSource {
    File(PathBuf),
    Url { url: String, pin: Option<[u8; 32]> },
}

impl ModuleSource {
    fn parse(token: OsString) -> Result<Self, String> {
        let text = token.to_string_lossy();
        if !text.starts_with("https://") && !text.starts_with("http://") {
            return Ok(Self::File(PathBuf::from(token)));
        }
        let mut url =
            reqwest::Url::parse(&text).map_err(|error| format!("cannot parse {text}: {error}"))?;
        let pin = url
            .fragment()
            .map(parse_digest)
            .transpose()
            .map_err(|error| format!("{text}: pinned digest is invalid: {error}"))?;
        url.set_fragment(None);
        Ok(Self::Url {
            url: url.into(),
            pin,
        })
    }

    async fn load(&self) -> Result<ModuleObject, String> {
        let object = match self {
            Self::File(path) => ModuleObject::read(path)?,
            Self::Url { url, .. } => ModuleObject::fetch(url).await?,
        };
        if let Self::Url { pin: Some(pin), .. } = self
            && object.hash != *pin
        {
            return Err(format!(
                "digest mismatch: pinned {} but the bytes hash to {}",
                hex(pin),
                hex(&object.hash)
            ));
        }
        Ok(object)
    }
}

struct ModuleObject {
    bytes: Vec<u8>,
    hash: [u8; 32],
}

impl ModuleObject {
    fn read(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        if metadata.len() == 0 || metadata.len() > wire::MAX_OBJECT_BYTES {
            return Err(format!(
                "{} must be 1..={} bytes",
                path.display(),
                wire::MAX_OBJECT_BYTES
            ));
        }
        let bytes = std::fs::read(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if bytes.is_empty() || bytes.len() as u64 > wire::MAX_OBJECT_BYTES {
            return Err(format!("{} changed size while it was read", path.display()));
        }
        let hash = *blake3::hash(&bytes).as_bytes();
        Ok(Self { bytes, hash })
    }

    async fn fetch(url: &str) -> Result<Self, String> {
        let bytes = fetch_http(url, wire::MAX_OBJECT_BYTES).await?;
        let hash = *blake3::hash(&bytes).as_bytes();
        Ok(Self { bytes, hash })
    }
}

async fn fetch_http(url: &str, maximum: u64) -> Result<Vec<u8>, String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|error| format!("cannot parse {url}: {error}"))?;
    if parsed.scheme() == "http" && !loopback_url(&parsed) {
        return Err(format!(
            "{url}: refusing plain HTTP to a non-loopback host; use https://"
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("yas/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("cannot build an HTTP client: {error}"))?;
    let mut response = client
        .get(parsed)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await
        .map_err(|error| format!("cannot fetch {url}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("cannot fetch {url}: {error}"))?;
    if response.url().scheme() == "http" && !loopback_url(response.url()) {
        return Err(format!(
            "{url}: redirected to plain HTTP ({}); refusing",
            response.url()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > maximum)
    {
        return Err(format!("{url} declares more than {} bytes", maximum));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("cannot read {url}: {error}"))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > maximum {
            return Err(format!("{url} exceeds the {}-byte download limit", maximum));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(format!("{url} returned no bytes"));
    }
    Ok(bytes)
}

fn parse_digest(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "expected a 64-hex-digit BLAKE3 digest, got {text:?}"
        ));
    }
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|error| format!("invalid BLAKE3 digest: {error}"))?;
    }
    Ok(digest)
}

fn loopback_url(url: &reqwest::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_never_infer_numeric_names() {
        assert!(matches!(
            selector("builder").unwrap(),
            Selector::Name("builder")
        ));
        assert!(matches!(
            selector("0123456789abcdef").unwrap(),
            Selector::Name("0123456789abcdef")
        ));
        assert!(matches!(
            selector("id:0123456789abcdef").unwrap(),
            Selector::Handle(0x0123_4567_89ab_cdef)
        ));
    }

    #[test]
    fn an_option_after_the_module_is_refused_rather_than_handed_to_the_extension() {
        let error = split_invocation(vec![
            OsString::from("mod.js"),
            OsString::from("--restart"),
            OsString::from("always"),
        ])
        .expect_err("a swallowed policy must not read as success");
        assert!(error.contains("--restart"), "{error}");

        let (_, args) = split_invocation(vec![
            OsString::from("mod.js"),
            OsString::from("--"),
            OsString::from("--restart"),
            OsString::from("always"),
        ])
        .expect("`--` means the extension really wants it");
        assert_eq!(args, ["--restart", "always"]);

        let (_, plain) =
            split_invocation(vec![OsString::from("mod.js"), OsString::from("--verbose")])
                .expect("an option this command does not own belongs to the extension");
        assert_eq!(plain, ["--verbose"]);
    }

    #[test]
    fn digest_pin_is_exact() {
        let digest = parse_digest(&"ab".repeat(32)).unwrap();
        assert_eq!(digest, [0xab; 32]);
        assert!(parse_digest("ab").is_err());
    }
}
