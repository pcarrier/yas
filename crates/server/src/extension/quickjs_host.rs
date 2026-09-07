//! Native QuickJS implementation of the extension guest contract.
//!
//! JavaScript source is compiled eagerly on the attempt's dedicated thread,
//! then evaluated only after the native YAS HELLO and mandatory attempt
//! context complete. I/O uses the same bounded stream handoffs and typed
//! `yas-guest` family helpers as a Wasmi guest.

use super::wasmi_host::{
    AttemptCancellation, AttemptFailure, AttemptOutcome, AttemptShared, FailureKind, HostBridge,
    LifecycleError, NativeHost, WasmiHostConfig, new_native_attempt_shared,
};
use crate::thread_name::{ThreadNames, extension_thread_names};
use rquickjs::{
    Array, BigInt, CatchResultExt, Context as JsContext, Ctx, Function, Module, Object, Runtime,
    TypedArray, Value, WriteOptions, function::Func, promise::MaybePromise,
};
use std::{
    cell::RefCell,
    cmp, fmt,
    rc::Rc,
    sync::{Arc, MutexGuard, atomic::Ordering},
    thread,
    time::Duration,
};
use tokio::sync::oneshot;
use yas_guest::{
    Client, MonotonicInstant, WaitOutcome,
    command::{CommandProvider, Input as CommandInput, Invocation, ProviderEvent},
    native_host,
    process::{Event as ProcessEvent, Process, StreamKind},
};
use yas_wire::{
    Class, Decode, Extensions, core::Status, family, fs as fs_wire, git as git_wire,
    net as net_wire, process as process_wire,
};

const SOURCE_NAME: &str = "extension.js";
const RANDOM_MAX_BYTES: usize = 16 * 1024 * 1024;
const QUICKJS_PROCESS_STREAM_WINDOW: u64 = 4 * 1024 * 1024;
const QUICKJS_PROCESS_CAPTURE_BYTES: usize = 1024 * 1024;
const QUICKJS_FS_INDEX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AttemptSpec {
    pub source: Arc<[u8]>,
    pub module_hash: [u8; 32],
    pub extension_id: u64,
    pub label: Option<String>,
    pub config: WasmiHostConfig,
}

#[derive(Debug)]
pub enum SpawnError {
    InvalidConfig(super::wasmi_host::ConfigError),
    InvalidExtensionId,
    Thread(std::io::Error),
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(error) => write!(f, "invalid QuickJS host configuration: {error}"),
            Self::InvalidExtensionId => f.write_str("extension ID must be non-zero"),
            Self::Thread(error) => write!(f, "failed to spawn extension thread: {error}"),
        }
    }
}

impl std::error::Error for SpawnError {}

/// Owner of one dedicated native QuickJS attempt thread.
#[derive(Debug)]
pub struct QuickJsAttempt {
    names: ThreadNames,
    shared: Arc<AttemptShared>,
    bridge: HostBridge,
    prepared_rx: Option<oneshot::Receiver<Result<(), AttemptFailure>>>,
    prepared: bool,
    started: bool,
    thread: Option<thread::JoinHandle<AttemptOutcome>>,
}

impl QuickJsAttempt {
    pub fn thread_names(&self) -> &ThreadNames {
        &self.names
    }

    pub fn cancellation(&self) -> AttemptCancellation {
        AttemptCancellation {
            inner: Arc::clone(&self.shared),
        }
    }

    pub fn bridge(&self) -> HostBridge {
        self.bridge.clone()
    }

    pub async fn wait_prepared(&mut self) -> Result<(), AttemptFailure> {
        let receiver = self.prepared_rx.take().ok_or_else(|| {
            AttemptFailure::new(
                FailureKind::HostFailure,
                LifecycleError::PreparationAlreadyObserved.to_string(),
            )
        })?;
        let result = receiver.await.map_err(|_| {
            AttemptFailure::new(
                FailureKind::HostFailure,
                LifecycleError::PreparationChannelClosed.to_string(),
            )
        })?;
        if result.is_ok() {
            self.prepared = true;
        }
        result
    }

    pub fn start(&mut self) -> Result<(), LifecycleError> {
        if !self.prepared {
            return Err(LifecycleError::NotPrepared);
        }
        if self.started {
            return Err(LifecycleError::AlreadyStarted);
        }
        self.started = true;
        *lock_unpoison(&self.shared.start) = true;
        self.shared.start_cv.notify_all();
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation().cancel();
    }

    pub async fn join(mut self) -> Result<AttemptOutcome, LifecycleError> {
        let handle = self.thread.take().ok_or(LifecycleError::JoinAlreadyTaken)?;
        tokio::task::spawn_blocking(move || {
            handle.join().map_err(|_| LifecycleError::ThreadPanicked)
        })
        .await
        .map_err(|_| LifecycleError::JoinTaskCancelled)?
    }
}

impl Drop for QuickJsAttempt {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

pub fn spawn_attempt(spec: AttemptSpec) -> Result<QuickJsAttempt, SpawnError> {
    spec.config.validate().map_err(SpawnError::InvalidConfig)?;
    if spec.extension_id == 0 {
        return Err(SpawnError::InvalidExtensionId);
    }
    let names = extension_thread_names(spec.label.as_deref(), &spec.module_hash, spec.extension_id);
    let shared = new_native_attempt_shared();
    let bridge = HostBridge {
        shared: Arc::clone(&shared),
    };
    let (prepared_tx, prepared_rx) = oneshot::channel();
    let thread_shared = Arc::clone(&shared);
    let stack_size = spec.config.native_stack_bytes;
    let thread = thread::Builder::new()
        .name(names.os.clone())
        .stack_size(stack_size)
        .spawn(move || attempt_thread(spec, thread_shared, prepared_tx))
        .map_err(SpawnError::Thread)?;
    Ok(QuickJsAttempt {
        names,
        shared,
        bridge,
        prepared_rx: Some(prepared_rx),
        prepared: false,
        started: false,
        thread: Some(thread),
    })
}

/// Compile JavaScript without evaluating it. Upload admission uses this on the
/// same bounded validation pool as Wasmi translation.
pub fn validate_source(source: &[u8], config: &WasmiHostConfig) -> Result<(), AttemptFailure> {
    config
        .validate()
        .map_err(|error| AttemptFailure::new(FailureKind::Validation, error.to_string()))?;
    let source = source_text(source)?;
    let (_runtime, _context, _bytecode) = prepare_runtime(source, config, None)?;
    Ok(())
}

fn attempt_thread(
    spec: AttemptSpec,
    shared: Arc<AttemptShared>,
    prepared_tx: oneshot::Sender<Result<(), AttemptFailure>>,
) -> AttemptOutcome {
    let source = match source_text(&spec.source) {
        Ok(source) => source,
        Err(error) => {
            let _ = prepared_tx.send(Err(error.clone()));
            shared.io.abort_handoffs();
            return AttemptOutcome::Failed(error);
        }
    };
    let runner = match PreparedRunner::new(source, &spec.config, Arc::clone(&shared)) {
        Ok(runner) => runner,
        Err(error) => {
            let _ = prepared_tx.send(Err(error.clone()));
            shared.io.abort_handoffs();
            return AttemptOutcome::Failed(error);
        }
    };
    if prepared_tx.send(Ok(())).is_err() {
        shared.io.abort_handoffs();
        return AttemptOutcome::Cancelled;
    }
    let mut started = lock_unpoison(&shared.start);
    while !*started && !shared.io.cancelled.load(Ordering::Acquire) {
        started = shared
            .start_cv
            .wait(started)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    drop(started);
    if shared.io.cancelled.load(Ordering::Acquire) {
        shared.io.abort_handoffs();
        return AttemptOutcome::Cancelled;
    }
    let outcome = runner.run();
    match &outcome {
        AttemptOutcome::Returned(_) => {
            shared.io.outgoing.seal_producer();
            shared.io.incoming.close_consumer();
        }
        AttemptOutcome::Cancelled | AttemptOutcome::Failed(_) => shared.io.abort_handoffs(),
    }
    outcome
}

struct PreparedRunner {
    runtime: Runtime,
    context: JsContext,
    bytecode: Vec<u8>,
    shared: Arc<AttemptShared>,
}

impl PreparedRunner {
    fn new(
        source: &str,
        config: &WasmiHostConfig,
        shared: Arc<AttemptShared>,
    ) -> Result<Self, AttemptFailure> {
        let (runtime, context, bytecode) =
            prepare_runtime(source, config, Some(Arc::clone(&shared)))?;
        Ok(Self {
            runtime,
            context,
            bytecode,
            shared,
        })
    }

    fn run(self) -> AttemptOutcome {
        let Self {
            runtime,
            context,
            bytecode,
            shared,
        } = self;
        let _host = native_host::install(NativeHost::new(Arc::clone(&shared.io)));
        let client = match Client::bootstrap() {
            Ok(client) => Rc::new(RefCell::new(QuickJsGuest::new(client))),
            Err(error) => {
                return AttemptOutcome::Failed(AttemptFailure::new(
                    FailureKind::AbiMisuse,
                    format!("QuickJS bootstrap failed: {error}"),
                ));
            }
        };
        let result = context.with(|ctx| {
            let result = (|| {
                install_bindings(&ctx, Rc::clone(&client))?;
                // The bytes were produced by this exact QuickJS runtime during
                // preparation and have not crossed a trust boundary.
                let module = unsafe { Module::load(ctx.clone(), &bytecode)? };
                let (module, evaluated) = module.eval()?;
                evaluated.finish::<()>()?;
                let default = module.get::<_, Option<Function>>("default")?;
                let Some(default) = default else {
                    return Ok(0);
                };
                let returned = default.call::<_, MaybePromise>(())?;
                let returned = returned.finish::<Value>()?;
                if returned.is_undefined() {
                    return Ok(0);
                }
                returned.as_int().ok_or_else(|| {
                    rquickjs::Exception::throw_type(
                        &ctx,
                        "default export must return an i32 or undefined",
                    )
                })
            })();
            result.catch(&ctx).map_err(|error| error.to_string())
        });
        drop(client);
        drop(context);
        drop(runtime);
        match result {
            Ok(code) if shared.io.cancelled.load(Ordering::Acquire) => {
                let _ = code;
                AttemptOutcome::Cancelled
            }
            Ok(code) => AttemptOutcome::Returned(code),
            Err(_) if shared.io.cancelled.load(Ordering::Acquire) => AttemptOutcome::Cancelled,
            Err(detail) => AttemptOutcome::Failed(AttemptFailure::new(
                FailureKind::Trap,
                format!("QuickJS exception: {detail}"),
            )),
        }
    }
}

struct QuickJsGuest {
    client: Client,
    command_provider: Option<CommandProvider>,
    invocation: Option<Invocation>,
}

impl QuickJsGuest {
    fn new(client: Client) -> Self {
        Self {
            client,
            command_provider: None,
            invocation: None,
        }
    }

    fn register_command(&mut self, descriptor: &str) -> Result<(), yas_guest::command::Error> {
        if self.command_provider.is_some() {
            return Err(yas_guest::command::Error::InvalidContext);
        }
        let context = self.client.context();
        let listener_name = format!(
            "yas.cli.{:016x}.{}",
            context.extension_handle, context.attempt
        );
        let listener = self.client.listen_channel(&listener_name, &[])?;
        self.command_provider = Some(CommandProvider::register(
            &mut self.client,
            listener,
            descriptor,
        )?);
        Ok(())
    }

    fn accept_command(
        &mut self,
    ) -> Result<Option<(yas_guest::command::InvocationRequest, u64)>, yas_guest::command::Error>
    {
        if self.invocation.is_some() {
            return Err(yas_guest::command::Error::InvalidInvocation(
                "previous invocation is still active",
            ));
        }
        let provider = self
            .command_provider
            .as_mut()
            .ok_or(yas_guest::command::Error::InvalidContext)?;
        match provider.accept(&mut self.client)? {
            ProviderEvent::Invocation(invocation) => {
                let request = invocation.request().clone();
                let channel_handle = invocation.channel_handle();
                self.invocation = Some(*invocation);
                Ok(Some((request, channel_handle)))
            }
            ProviderEvent::Closed(_) => Ok(None),
        }
    }

    fn command_stdout(&mut self, data: &[u8]) -> Result<(), yas_guest::command::Error> {
        let Self {
            client, invocation, ..
        } = self;
        invocation
            .as_mut()
            .ok_or(yas_guest::command::Error::InvalidInvocation(
                "there is no active invocation",
            ))?
            .stdout(client, data)
    }

    fn command_stderr(&mut self, data: &[u8]) -> Result<(), yas_guest::command::Error> {
        let Self {
            client, invocation, ..
        } = self;
        invocation
            .as_mut()
            .ok_or(yas_guest::command::Error::InvalidInvocation(
                "there is no active invocation",
            ))?
            .stderr(client, data)
    }

    fn command_result(
        &mut self,
        content_type: &str,
        data: &[u8],
    ) -> Result<(), yas_guest::command::Error> {
        let Self {
            client, invocation, ..
        } = self;
        invocation
            .as_mut()
            .ok_or(yas_guest::command::Error::InvalidInvocation(
                "there is no active invocation",
            ))?
            .result(client, content_type, data)
    }

    fn command_exit(&mut self, code: i32, detail: &str) -> Result<(), yas_guest::command::Error> {
        let mut invocation =
            self.invocation
                .take()
                .ok_or(yas_guest::command::Error::InvalidInvocation(
                    "there is no active invocation",
                ))?;
        invocation.exit(&mut self.client, code, detail)
    }

    fn command_cancel(&mut self) -> Result<(), yas_guest::command::Error> {
        let mut invocation =
            self.invocation
                .take()
                .ok_or(yas_guest::command::Error::InvalidInvocation(
                    "there is no active invocation",
                ))?;
        invocation.cancel(&mut self.client)
    }

    fn command_read_stdin(&mut self, maximum: usize) -> Result<Vec<u8>, String> {
        let Self {
            client, invocation, ..
        } = self;
        let invocation = invocation
            .as_mut()
            .ok_or_else(|| "there is no active invocation".to_owned())?;
        if !invocation.request().streams_stdin {
            return Err("the active invocation does not stream stdin".to_owned());
        }
        let mut bytes = Vec::new();
        loop {
            match invocation
                .receive_input(client)
                .map_err(|error| error.to_string())?
            {
                CommandInput::Stdin(chunk) => {
                    if bytes.len().saturating_add(chunk.len()) > maximum {
                        return Err(format!("command stdin exceeds {maximum} bytes"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                CommandInput::StdinEof => return Ok(bytes),
                CommandInput::Cancel | CommandInput::Closed(_) => {
                    return Err("command invocation was cancelled".to_owned());
                }
            }
        }
    }

    fn supports(&self, family_id: u16, class: u8, kind: u16) -> bool {
        let class = match class {
            yas_wire::schema::transport::class::EVENT => Class::Event,
            yas_wire::schema::transport::class::REQUEST => Class::Request,
            yas_wire::schema::transport::class::RESULT => Class::Result,
            _ => return false,
        };
        self.client.supports(family_id, class, kind)
    }

    fn channel_message_limit(&self) -> u64 {
        self.client
            .family(family::CHANNEL)
            .and_then(|descriptor| {
                yas_wire::channel::Limits::from_extensions(&descriptor.limits).ok()
            })
            .map_or(yas_wire::channel::MAX_MESSAGE_BYTES, |limits| {
                limits.max_message_bytes
            })
    }

    fn environment_json(&mut self) -> Result<String, String> {
        let entries = self
            .client
            .get_environment()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|entry| {
                serde_json::json!({
                    "key": String::from_utf8_lossy(&entry.key),
                    "value": String::from_utf8_lossy(&entry.value),
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&entries).map_err(|error| error.to_string())
    }

    fn run_process_json(&mut self, request_json: &str) -> Result<String, String> {
        let request: serde_json::Value =
            serde_json::from_str(request_json).map_err(|error| error.to_string())?;
        let object = request
            .as_object()
            .ok_or_else(|| "process request must be an object".to_owned())?;
        let operation_id = object
            .get("operationId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "process operationId is required".to_owned())
            .and_then(parse_operation_id)?;
        let cwd = match object.get("cwd") {
            None | Some(serde_json::Value::Null) => process_wire::Cwd::ServerDefault,
            Some(value) => process_wire::Cwd::Path(
                value
                    .as_str()
                    .ok_or_else(|| "process cwd must be a string or null".to_owned())?
                    .as_bytes()
                    .to_vec(),
            ),
        };
        let argv = object
            .get("argv")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "process argv is required".to_owned())?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|value| value.as_bytes().to_vec())
                    .ok_or_else(|| "process argv must contain strings".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if argv.is_empty() {
            return Err("process argv must not be empty".to_owned());
        }
        let env = object
            .get("env")
            .and_then(serde_json::Value::as_object)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| {
                        value
                            .as_str()
                            .map(|value| process_wire::EnvEntry {
                                key: key.as_bytes().to_vec(),
                                value: value.as_bytes().to_vec(),
                            })
                            .ok_or_else(|| "process env values must be strings".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        let deadline = object
            .get("deadlineNanos")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "process deadlineNanos is required".to_owned())?
            .parse::<i64>()
            .map(MonotonicInstant::from_raw_nanos)
            .map_err(|_| "process deadlineNanos is invalid".to_owned())?;

        let mut process = self
            .client
            .spawn_process_with_operation_id_and_window(
                operation_id,
                0,
                process_wire::EnvironmentKind::Session,
                cwd,
                argv,
                env,
                Extensions::default(),
                QUICKJS_PROCESS_STREAM_WINDOW,
            )
            .map_err(|error| format!("Process SPAWN: {error}"))?;
        process
            .close_stdin(&mut self.client)
            .map_err(|error| format!("Process stdin: {error}"))?;
        let output = collect_quickjs_process(
            &mut self.client,
            self.invocation.as_mut(),
            &mut process,
            operation_id,
            deadline,
        )?;
        serde_json::to_string(&serde_json::json!({
            "code": output.code,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
        .map_err(|error| error.to_string())
    }

    fn git_inspect_json(
        &mut self,
        path: &str,
        query: &str,
        argument: Option<&str>,
    ) -> Result<String, String> {
        let mut repository = self
            .client
            .open_git(git_wire::RepositorySource::PlatformPath(
                path.as_bytes().to_vec(),
            ))
            .map_err(|error| format!("Git OPEN: {error}"))?;
        let result = match query {
            "rebase" => repository
                .state_snapshot(
                    &mut self.client,
                    yas_wire::schema::git::WATCH_OPERATION as u16,
                )
                .map_err(|error| format!("Git WATCH OPERATION: {error}"))
                .and_then(|entities| {
                    serde_json::to_string(&entities.into_iter().any(|entity| {
                        matches!(
                            entity.body,
                            git_wire::EntityBody::Operation(operation)
                                if operation.operation_kind
                                    == yas_wire::schema::git::OPERATION_REBASE as u8
                        )
                    }))
                    .map_err(|error| error.to_string())
                }),
            "status" => repository
                .state_snapshot(&mut self.client, yas_wire::schema::git::WATCH_STATUS as u16)
                .map_err(|error| format!("Git WATCH STATUS: {error}"))
                .and_then(|entities| {
                    let dirty = !entities.is_empty();
                    let mut conflicted = entities
                        .into_iter()
                        .filter_map(|entity| {
                            let git_wire::EntityBody::Status(status) = entity.body else {
                                return None;
                            };
                            let unmerged = status.flags
                                & yas_wire::schema::git::STATE_STATUS_CONFLICTED as u16
                                != 0
                                || status.index_status
                                    == yas_wire::schema::git::WORKTREE_STATUS_UNMERGED as u8
                                || status.worktree_status
                                    == yas_wire::schema::git::WORKTREE_STATUS_UNMERGED as u8;
                            unmerged.then(|| {
                                fs_wire::Path::decode(&entity.key)
                                    .map(|path| native_path_text(&path))
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string())?;
                    conflicted.sort();
                    conflicted.dedup();
                    serde_json::to_string(&serde_json::json!({
                        "dirty": dirty,
                        "conflicted": conflicted,
                    }))
                    .map_err(|error| error.to_string())
                }),
            "resolve" => {
                let spec = argument.ok_or_else(|| "Git resolve needs an argument".to_owned())?;
                repository
                    .resolve(&mut self.client, spec.as_bytes().to_vec())
                    .map_err(|error| format!("Git QUERY RESOLVE {spec}: {error}"))
                    .and_then(|object| {
                        serde_json::to_string(&object.map(|object| hex_bytes(&object.bytes)))
                            .map_err(|error| error.to_string())
                    })
            }
            _ => Err(format!("unknown Git inspection query: {query}")),
        };
        let close = repository
            .close(&mut self.client)
            .map_err(|error| format!("Git CLOSE: {error}"));
        match (result, close) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }

    fn fs_index_files_json(&mut self, path: &str) -> Result<String, String> {
        let mut root = self
            .client
            .open_fs(
                fs_wire::RootSource::PlatformPath(path.as_bytes().to_vec()),
                0,
            )
            .map_err(|error| format!("FS OPEN: {error}"))?;
        let result = (|| {
            let mut cursor = Vec::new();
            let mut paths = Vec::new();
            let mut path_bytes = 0usize;
            loop {
                let page = root
                    .index(
                        &mut self.client,
                        (yas_wire::schema::fs::INDEX_INCLUDE_FILES
                            | yas_wire::schema::fs::INDEX_INCLUDE_IGNORED)
                            as u16,
                        0,
                        cursor,
                    )
                    .map_err(|error| format!("FS INDEX: {error}"))?;
                for record in page.records {
                    if let fs_wire::QueryRecord::Path(record) = record {
                        let path = native_path_text(&record.path);
                        path_bytes = path_bytes.saturating_add(path.len());
                        if path_bytes > QUICKJS_FS_INDEX_BYTES {
                            return Err("FS index exceeds the 16 MiB QuickJS limit".to_owned());
                        }
                        paths.push(path);
                    }
                }
                if page.next_cursor.is_empty() {
                    break;
                }
                cursor = page.next_cursor;
            }
            paths.sort();
            serde_json::to_string(&paths).map_err(|error| error.to_string())
        })();
        let close = root
            .close(&mut self.client)
            .map_err(|error| format!("FS CLOSE: {error}"));
        match (result, close) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }

    fn fs_read(
        &mut self,
        root_path: &str,
        relative: &str,
        maximum: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        let path = relative_path(relative)?;
        let mut root = self
            .client
            .open_fs(
                fs_wire::RootSource::PlatformPath(root_path.as_bytes().to_vec()),
                0,
            )
            .map_err(|error| format!("FS OPEN: {error}"))?;
        let result = root.fetch(&mut self.client, path, None, maximum);
        let close = root.close(&mut self.client);
        match (result, close) {
            (
                Err(yas_guest::fs::Error::Client(yas_guest::Error::RequestFailed {
                    status: Status::NotFound,
                    ..
                })),
                _,
            ) => Ok(None),
            (Err(error), _) => Err(format!("FS FETCH: {error}")),
            (Ok(_), Err(error)) => Err(format!("FS CLOSE: {error}")),
            (Ok(content), Ok(())) => Ok(Some(content.bytes)),
        }
    }

    fn fs_write(
        &mut self,
        operation_id: [u8; 16],
        root_path: &str,
        relative: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let path = relative_path(relative)?;
        let mut root = self
            .client
            .open_fs(
                fs_wire::RootSource::PlatformPath(root_path.as_bytes().to_vec()),
                0,
            )
            .map_err(|error| format!("FS OPEN: {error}"))?;
        let result = root
            .stage_write(
                &mut self.client,
                path,
                fs_wire::Precondition::Any,
                0,
                0o600,
                bytes,
            )
            .map_err(|error| format!("FS STAGE_WRITE: {error}"))
            .and_then(|mut staged| {
                staged
                    .commit_with_operation_id(&mut self.client, operation_id, 0)
                    .map(|_| ())
                    .map_err(|error| format!("FS COMMIT: {error}"))
            });
        let close = root
            .close(&mut self.client)
            .map_err(|error| format!("FS CLOSE: {error}"));
        match (result, close) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn net_exchange(
        &mut self,
        host: String,
        port: u16,
        tls: bool,
        server_name: String,
        request: &[u8],
        maximum_response: usize,
        deadline: MonotonicInstant,
    ) -> Result<Vec<u8>, String> {
        if maximum_response == 0 || maximum_response > RANDOM_MAX_BYTES * 2 {
            return Err("Net response limit must be between 1 and 32 MiB".to_owned());
        }
        let tls_options = tls.then_some(net_wire::TlsOptions {
            verification: net_wire::TlsVerification::Strict,
            sni: server_name,
            alpn: vec![b"http/1.1".to_vec()],
            extensions: Extensions::default(),
        });
        let receive_window = (maximum_response as u64).clamp(1, QUICKJS_PROCESS_STREAM_WINDOW);
        let mut flow = self
            .client
            .open_byte_flow_window_until(
                net_wire::Address::Tcp { host, port },
                tls_options,
                Vec::new(),
                deadline,
                receive_window,
            )
            .map_err(|error| format!("Net OPEN: {error}"))?;
        let result = exchange_bytes(
            &mut self.client,
            self.invocation.as_mut(),
            &mut flow,
            request,
            maximum_response,
            deadline,
        );
        let close = flow
            .close(&mut self.client)
            .map_err(|error| format!("Net CLOSE: {error}"));
        match (result, close) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(bytes), Ok(())) => Ok(bytes),
        }
    }
}

struct QuickJsProcessOutput {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn collect_quickjs_process(
    client: &mut Client,
    mut invocation: Option<&mut Invocation>,
    process: &mut Process,
    operation_id: [u8; 16],
    deadline: MonotonicInstant,
) -> Result<QuickJsProcessOutput, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_closed = false;
    let mut stderr_closed = false;
    let mut exit = None;
    let mut stopping: Option<&'static str> = None;
    let mut cleanup_deadline = deadline;
    let mut killed = false;

    while exit.is_none() || !stdout_closed || !stderr_closed {
        let now = client.monotonic_now();
        if stopping.is_none() && now >= deadline {
            stopping = Some("process deadline exceeded");
        }
        if let Some(active) = invocation.as_deref_mut() {
            while let Some(input) = active
                .poll_input(client)
                .map_err(|error| error.to_string())?
            {
                if matches!(input, CommandInput::Cancel | CommandInput::Closed(_)) {
                    stopping = Some("command invocation was cancelled");
                }
            }
        }
        if stopping.is_some() && cleanup_deadline == deadline {
            let _ = process.control_with_operation_id(
                client,
                derived_control_id(operation_id, 0xa5),
                process_wire::ControlAction::Terminate,
                0,
            );
            cleanup_deadline = now + Duration::from_secs(5);
        } else if stopping.is_some() && now >= cleanup_deadline && !killed {
            let _ = process.control_with_operation_id(
                client,
                derived_control_id(operation_id, 0x5a),
                process_wire::ControlAction::Kill,
                0,
            );
            cleanup_deadline = now + Duration::from_secs(2);
            killed = true;
        } else if stopping.is_some() && now >= cleanup_deadline && killed {
            return Err(stopping.unwrap_or("process stopped").to_owned());
        }

        if !stdout_closed || !stderr_closed {
            let wait_deadline = cmp::min(
                now + Duration::from_millis(50),
                if stopping.is_some() {
                    cleanup_deadline
                } else {
                    deadline
                },
            );
            if let Some(event) = process
                .next_event_until(client, wait_deadline)
                .map_err(|error| format!("Process stream: {error}"))?
            {
                match event {
                    ProcessEvent::Output(delivery) => {
                        let kind = delivery.kind();
                        let data = process
                            .consume(client, delivery)
                            .map_err(|error| format!("Process consume: {error}"))?;
                        append_process_output(
                            match kind {
                                StreamKind::Stdout => &mut stdout,
                                StreamKind::Stderr => &mut stderr,
                            },
                            &data,
                        );
                    }
                    ProcessEvent::StreamClosed(closed) => match closed.kind {
                        StreamKind::Stdout => stdout_closed = true,
                        StreamKind::Stderr => stderr_closed = true,
                    },
                    ProcessEvent::StdinCredit { .. } | ProcessEvent::StdinClosed { .. } => {}
                }
            }
        }
        if exit.is_none() {
            match process.wait(client, 1) {
                Ok(record) => exit = Some(record),
                Err(yas_guest::process::Error::Client(yas_guest::Error::RequestFailed {
                    status: Status::Timeout,
                    ..
                })) => {}
                Err(error) => return Err(format!("Process WAIT: {error}")),
            }
        }
    }

    if let Some(reason) = stopping {
        return Err(reason.to_owned());
    }
    let exit = exit.ok_or_else(|| "Process exited without a status".to_owned())?;
    Ok(QuickJsProcessOutput {
        code: exit.code,
        stdout,
        stderr,
    })
}

fn append_process_output(destination: &mut Vec<u8>, bytes: &[u8]) {
    let available = QUICKJS_PROCESS_CAPTURE_BYTES.saturating_sub(destination.len());
    destination.extend_from_slice(&bytes[..bytes.len().min(available)]);
}

fn derived_control_id(mut operation_id: [u8; 16], discriminator: u8) -> [u8; 16] {
    operation_id[0] ^= discriminator;
    if operation_id == [0; 16] {
        operation_id[15] = 1;
    }
    operation_id
}

fn parse_operation_id(value: &str) -> Result<[u8; 16], String> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("operationId must contain 32 lowercase hex digits".to_owned());
    }
    let mut output = [0; 16];
    for (index, byte) in output.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|_| "operationId must contain 32 lowercase hex digits".to_owned())?;
    }
    if output == [0; 16] {
        return Err("operationId must be nonzero".to_owned());
    }
    Ok(output)
}

fn native_path_text(path: &fs_wire::Path) -> String {
    path.components
        .iter()
        .map(|component| String::from_utf8_lossy(component))
        .collect::<Vec<_>>()
        .join("/")
}

fn relative_path(value: &str) -> Result<fs_wire::Path, String> {
    if value.is_empty() || value.starts_with('/') || value.as_bytes().contains(&0) {
        return Err("FS path must be a non-empty relative path".to_owned());
    }
    let components = value
        .split('/')
        .map(|component| {
            if component.is_empty() || component == "." || component == ".." {
                Err("FS path contains an unsafe component".to_owned())
            } else {
                Ok(component.as_bytes().to_vec())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(fs_wire::Path { components })
}

fn exchange_bytes(
    client: &mut Client,
    mut invocation: Option<&mut Invocation>,
    flow: &mut yas_guest::net::ByteFlow,
    request: &[u8],
    maximum_response: usize,
    deadline: MonotonicInstant,
) -> Result<Vec<u8>, String> {
    let mut sent = 0usize;
    let mut response = Vec::new();
    let mut read_closed = false;
    while sent < request.len() || !read_closed {
        if client.monotonic_now() >= deadline {
            return Err("Net exchange deadline exceeded".to_owned());
        }
        if let Some(active) = invocation.as_deref_mut() {
            while let Some(input) = active
                .poll_input(client)
                .map_err(|error| error.to_string())?
            {
                if matches!(input, CommandInput::Cancel | CommandInput::Closed(_)) {
                    return Err("command invocation was cancelled".to_owned());
                }
            }
        }
        if sent < request.len() {
            let available = usize::try_from(flow.available_write_credit()).unwrap_or(usize::MAX);
            if available != 0 {
                let end = request.len().min(sent.saturating_add(available));
                flow.write(client, &request[sent..end])
                    .map_err(|error| format!("Net write: {error}"))?;
                sent = end;
                if sent == request.len() {
                    flow.shutdown_write(client)
                        .map_err(|error| format!("Net shutdown: {error}"))?;
                }
                continue;
            }
        }

        let event_deadline = cmp::min(client.monotonic_now() + Duration::from_millis(50), deadline);
        let Some(event) = flow
            .next_event_until(client, event_deadline)
            .map_err(|error| format!("Net read: {error}"))?
        else {
            continue;
        };
        match event {
            yas_guest::net::Event::Read(delivery) => {
                let bytes = flow
                    .consume(client, delivery)
                    .map_err(|error| format!("Net consume: {error}"))?;
                if response.len().saturating_add(bytes.len()) > maximum_response {
                    return Err(format!("Net response exceeds {maximum_response} bytes"));
                }
                response.extend_from_slice(&bytes);
            }
            yas_guest::net::Event::ReadClosed { status, detail } => {
                if Status::from_code(status) != Status::Ok {
                    return Err(format!("Net peer closed: {detail}"));
                }
                read_closed = true;
                if sent < request.len() {
                    return Err("Net peer closed before the request was written".to_owned());
                }
            }
            yas_guest::net::Event::Reset { status, detail } => {
                return Err(format!(
                    "Net flow reset with {:?}: {detail}",
                    Status::from_code(status)
                ));
            }
            yas_guest::net::Event::WriteCredit { .. } => {}
        }
    }
    Ok(response)
}

fn prepare_runtime(
    source: &str,
    config: &WasmiHostConfig,
    shared: Option<Arc<AttemptShared>>,
) -> Result<(Runtime, JsContext, Vec<u8>), AttemptFailure> {
    let runtime = Runtime::new().map_err(|error| {
        AttemptFailure::new(
            FailureKind::Instantiation,
            format!("create QuickJS runtime: {error}"),
        )
    })?;
    runtime.set_memory_limit(config.memory_bytes);
    runtime.set_max_stack_size(config.value_stack_bytes);
    if let Some(shared) = shared {
        runtime.set_interrupt_handler(Some(Box::new(move || {
            shared.io.cancelled.load(Ordering::Acquire)
        })));
    }
    let context = JsContext::full(&runtime).map_err(|error| {
        AttemptFailure::new(
            FailureKind::Instantiation,
            format!("create QuickJS context: {error}"),
        )
    })?;
    let bytecode = context.with(|ctx| {
        let result = Module::declare(ctx.clone(), SOURCE_NAME, source)
            .and_then(|module| module.write(WriteOptions::default()));
        result.catch(&ctx).map_err(|error| error.to_string())
    });
    let bytecode = bytecode.map_err(|detail| {
        AttemptFailure::new(
            FailureKind::Validation,
            format!("compile QuickJS source: {detail}"),
        )
    })?;
    Ok((runtime, context, bytecode))
}

fn source_text(source: &[u8]) -> Result<&str, AttemptFailure> {
    std::str::from_utf8(source).map_err(|error| {
        AttemptFailure::new(
            FailureKind::Validation,
            format!("QuickJS source is not UTF-8: {error}"),
        )
    })
}

fn install_bindings<'js>(
    ctx: &Ctx<'js>,
    client: Rc<RefCell<QuickJsGuest>>,
) -> rquickjs::Result<()> {
    let yas = Object::new(ctx.clone())?;
    let context = Object::new(ctx.clone())?;
    let guest = client.borrow();
    let info = guest.client.context();
    context.set(
        "extensionHandle",
        BigInt::from_u64(ctx.clone(), info.extension_handle)?,
    )?;
    context.set(
        "generation",
        BigInt::from_u64(ctx.clone(), info.generation)?,
    )?;
    context.set(
        "definitionRevision",
        BigInt::from_u64(ctx.clone(), info.definition_revision)?,
    )?;
    context.set("attempt", BigInt::from_u64(ctx.clone(), info.attempt)?)?;
    context.set("taskId", info.task_id)?;
    context.set("contentHash", hex_hash(&info.content_hash))?;
    context.set("name", info.name.clone())?;
    let args = Array::new(ctx.clone())?;
    for (index, argument) in info.argv.iter().enumerate() {
        args.set(index, String::from_utf8_lossy(argument).as_ref())?;
    }
    context.set("argv", args)?;
    context.set(
        "detached",
        info.flags & yas_wire::schema::extension::DEFINITION_DETACHED as u16 != 0,
    )?;
    context.set(
        "persistent",
        info.flags & yas_wire::schema::extension::DEFINITION_PERSISTENT as u16 != 0,
    )?;
    context.set(
        "enabled",
        info.flags & yas_wire::schema::extension::DEFINITION_ENABLED as u16 != 0,
    )?;
    context.set(
        "desiredRunning",
        info.flags & yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING as u16 != 0,
    )?;
    let hello = guest.client.hello();
    context.set("protocolMinor", hello.minor)?;
    context.set("bootId", hex_bytes(&hello.boot_id))?;
    context.set("sessionId", hex_bytes(&hello.session_id))?;
    context.set("serverName", hello.server_name.clone())?;
    context.set("serverRelease", hello.server_release.clone())?;
    let families = Array::new(ctx.clone())?;
    for (index, family) in hello.families.iter().enumerate() {
        families.set(index, u32::from(family.family_id))?;
    }
    context.set("families", families)?;
    drop(guest);
    yas.set("context", context)?;

    let register_client = Rc::clone(&client);
    yas.set(
        "registerCommand",
        Func::from(move |ctx: Ctx<'js>, descriptor: String| {
            register_client
                .borrow_mut()
                .register_command(&descriptor)
                .map_err(|error| js_error(&ctx, "registerCommand", error))
        }),
    )?;

    let accept_client = Rc::clone(&client);
    yas.set(
        "acceptCommand",
        Func::from(move |ctx: Ctx<'js>| {
            let request = accept_client
                .borrow_mut()
                .accept_command()
                .map_err(|error| js_error(&ctx, "acceptCommand", error))?;
            request
                .map(|(request, channel_handle)| {
                    let value = Object::new(ctx.clone())?;
                    let args = Array::new(ctx.clone())?;
                    for (index, argument) in request.args.iter().enumerate() {
                        args.set(index, argument.as_str())?;
                    }
                    value.set("args", args)?;
                    value.set("streamsStdin", request.streams_stdin)?;
                    value.set(
                        "channelHandle",
                        BigInt::from_u64(ctx.clone(), channel_handle)?,
                    )?;
                    Ok::<Object<'js>, rquickjs::Error>(value)
                })
                .transpose()
        }),
    )?;

    for (name, operation) in [("commandStdout", 0_u8), ("commandStderr", 1_u8)] {
        let output_client = Rc::clone(&client);
        yas.set(
            name,
            Func::from(move |ctx: Ctx<'js>, data: TypedArray<'js, u8>| {
                let result = if operation == 0 {
                    output_client.borrow_mut().command_stdout(data.as_ref())
                } else {
                    output_client.borrow_mut().command_stderr(data.as_ref())
                };
                result.map_err(|error| js_error(&ctx, name, error))
            }),
        )?;
    }

    let result_client = Rc::clone(&client);
    yas.set(
        "commandResult",
        Func::from(
            move |ctx: Ctx<'js>, content_type: String, data: TypedArray<'js, u8>| {
                result_client
                    .borrow_mut()
                    .command_result(&content_type, data.as_ref())
                    .map_err(|error| js_error(&ctx, "commandResult", error))
            },
        ),
    )?;

    let exit_client = Rc::clone(&client);
    yas.set(
        "commandExit",
        Func::from(move |ctx: Ctx<'js>, code: i32, detail: String| {
            exit_client
                .borrow_mut()
                .command_exit(code, &detail)
                .map_err(|error| js_error(&ctx, "commandExit", error))
        }),
    )?;

    let cancel_client = Rc::clone(&client);
    yas.set(
        "commandCancel",
        Func::from(move |ctx: Ctx<'js>| {
            cancel_client
                .borrow_mut()
                .command_cancel()
                .map_err(|error| js_error(&ctx, "commandCancel", error))
        }),
    )?;

    let stdin_client = Rc::clone(&client);
    yas.set(
        "commandReadStdin",
        Func::from(move |ctx: Ctx<'js>, maximum: u32| {
            let bytes = stdin_client
                .borrow_mut()
                .command_read_stdin(maximum as usize)
                .map_err(|error| js_error(&ctx, "commandReadStdin", error))?;
            TypedArray::new(ctx, bytes)
        }),
    )?;

    let supports_client = Rc::clone(&client);
    yas.set(
        "supports",
        Func::from(
            move |_ctx: Ctx<'js>, family_id: u32, class: u32, kind: u32| {
                let Ok(family_id) = u16::try_from(family_id) else {
                    return false;
                };
                let Ok(class) = u8::try_from(class) else {
                    return false;
                };
                let Ok(kind) = u16::try_from(kind) else {
                    return false;
                };
                supports_client.borrow().supports(family_id, class, kind)
            },
        ),
    )?;

    let channel_limit_client = Rc::clone(&client);
    yas.set(
        "channelMessageLimit",
        Func::from(move |ctx: Ctx<'js>| {
            BigInt::from_u64(ctx, channel_limit_client.borrow().channel_message_limit())
        }),
    )?;

    let environment_client = Rc::clone(&client);
    yas.set(
        "environmentJson",
        Func::from(move |ctx: Ctx<'js>| {
            environment_client
                .borrow_mut()
                .environment_json()
                .map_err(|error| js_error(&ctx, "environmentJson", error))
        }),
    )?;

    let process_client = Rc::clone(&client);
    yas.set(
        "runProcessJson",
        Func::from(move |ctx: Ctx<'js>, request: String| {
            process_client
                .borrow_mut()
                .run_process_json(&request)
                .map_err(|error| js_error(&ctx, "runProcessJson", error))
        }),
    )?;

    let git_client = Rc::clone(&client);
    yas.set(
        "gitInspectJson",
        Func::from(
            move |ctx: Ctx<'js>, path: String, query: String, argument: Option<String>| {
                git_client
                    .borrow_mut()
                    .git_inspect_json(&path, &query, argument.as_deref())
                    .map_err(|error| js_error(&ctx, "gitInspectJson", error))
            },
        ),
    )?;

    let fs_index_client = Rc::clone(&client);
    yas.set(
        "fsIndexFilesJson",
        Func::from(move |ctx: Ctx<'js>, path: String| {
            fs_index_client
                .borrow_mut()
                .fs_index_files_json(&path)
                .map_err(|error| js_error(&ctx, "fsIndexFilesJson", error))
        }),
    )?;

    let fs_read_client = Rc::clone(&client);
    yas.set(
        "fsRead",
        Func::from(
            move |ctx: Ctx<'js>, root: String, relative: String, maximum: u32| {
                let bytes = fs_read_client
                    .borrow_mut()
                    .fs_read(&root, &relative, u64::from(maximum))
                    .map_err(|error| js_error(&ctx, "fsRead", error))?;
                bytes
                    .map(|bytes| TypedArray::new(ctx.clone(), bytes))
                    .transpose()
            },
        ),
    )?;

    let fs_write_client = Rc::clone(&client);
    yas.set(
        "fsWrite",
        Func::from(
            move |ctx: Ctx<'js>,
                  operation_id: String,
                  root: String,
                  relative: String,
                  data: TypedArray<'js, u8>| {
                let operation_id = parse_operation_id(&operation_id)
                    .map_err(|error| js_error(&ctx, "fsWrite", error))?;
                fs_write_client
                    .borrow_mut()
                    .fs_write(operation_id, &root, &relative, data.as_ref())
                    .map_err(|error| js_error(&ctx, "fsWrite", error))
            },
        ),
    )?;

    yas.set(
        "blake3",
        Func::from(move |ctx: Ctx<'js>, data: TypedArray<'js, u8>| {
            TypedArray::new(ctx, blake3::hash(data.as_ref()).as_bytes().to_vec())
        }),
    )?;

    let net_client = Rc::clone(&client);
    yas.set(
        "netExchange",
        Func::from(
            move |ctx: Ctx<'js>,
                  connection_json: String,
                  request: TypedArray<'js, u8>,
                  maximum_response: u32,
                  deadline: BigInt<'js>| {
                let connection: serde_json::Value = serde_json::from_str(&connection_json)
                    .map_err(|error| js_error(&ctx, "netExchange", error))?;
                let host = connection
                    .get("host")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| js_error(&ctx, "netExchange", "host is required"))?
                    .to_owned();
                let port = connection
                    .get("port")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u16::try_from(value).ok())
                    .filter(|value| *value != 0)
                    .ok_or_else(|| js_error(&ctx, "netExchange", "port is invalid"))?;
                let tls = connection
                    .get("tls")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let server_name = connection
                    .get("serverName")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&host)
                    .to_owned();
                let deadline = deadline
                    .to_i64()
                    .map(MonotonicInstant::from_raw_nanos)
                    .map_err(|error| js_error(&ctx, "netExchange", error))?;
                let bytes = net_client
                    .borrow_mut()
                    .net_exchange(
                        host,
                        port,
                        tls,
                        server_name,
                        request.as_ref(),
                        maximum_response as usize,
                        deadline,
                    )
                    .map_err(|error| js_error(&ctx, "netExchange", error))?;
                TypedArray::new(ctx, bytes)
            },
        ),
    )?;

    let wait_client = Rc::clone(&client);
    yas.set(
        "wait",
        Func::from(move |ctx: Ctx<'js>| {
            wait_client
                .borrow()
                .client
                .wait()
                .map(wait_code)
                .map_err(|error| js_error(&ctx, "wait", error))
        }),
    )?;

    let wait_until_client = Rc::clone(&client);
    yas.set(
        "waitUntil",
        Func::from(move |ctx: Ctx<'js>, deadline: BigInt<'js>| {
            let deadline = deadline
                .to_i64()
                .map_err(|error| js_error(&ctx, "waitUntil", error))?;
            wait_until_client
                .borrow()
                .client
                .wait_until(MonotonicInstant::from_raw_nanos(deadline))
                .map(wait_code)
                .map_err(|error| js_error(&ctx, "waitUntil", error))
        }),
    )?;

    let realtime_client = Rc::clone(&client);
    yas.set(
        "realtimeNow",
        Func::from(move |ctx: Ctx<'js>| {
            BigInt::from_i64(
                ctx,
                realtime_client
                    .borrow()
                    .client
                    .realtime_now()
                    .unix_timestamp_nanos(),
            )
        }),
    )?;

    let monotonic_client = Rc::clone(&client);
    yas.set(
        "monotonicNow",
        Func::from(move |ctx: Ctx<'js>| {
            BigInt::from_i64(
                ctx,
                monotonic_client.borrow().client.monotonic_now().raw_nanos(),
            )
        }),
    )?;

    let random_client = Rc::clone(&client);
    yas.set(
        "random",
        Func::from(move |ctx: Ctx<'js>, length: u32| {
            let length = length as usize;
            if length > RANDOM_MAX_BYTES {
                return Err(rquickjs::Exception::throw_range(
                    &ctx,
                    "random length exceeds 16 MiB",
                ));
            }
            let mut bytes = vec![0; length];
            random_client
                .borrow()
                .client
                .random(&mut bytes)
                .map_err(|error| js_error(&ctx, "random", error))?;
            TypedArray::new(ctx, bytes)
        }),
    )?;

    let sleep_client = Rc::clone(&client);
    yas.set(
        "sleep",
        Func::from(move |ctx: Ctx<'js>, milliseconds: f64| {
            if !milliseconds.is_finite() || milliseconds < 0.0 {
                return Err(rquickjs::Exception::throw_range(
                    &ctx,
                    "sleep duration must be a finite non-negative number",
                ));
            }
            let duration = Duration::try_from_secs_f64(milliseconds / 1_000.0).map_err(|_| {
                rquickjs::Exception::throw_range(&ctx, "sleep duration is out of range")
            })?;
            sleep_client
                .borrow_mut()
                .client
                .sleep(duration)
                .map_err(|error| js_error(&ctx, "sleep", error))
        }),
    )?;

    let log_client = Rc::clone(&client);
    yas.set(
        "log",
        Func::from(move |ctx: Ctx<'js>, message: String| {
            if message.len() > yas_wire::schema::extension::MAX_OUTPUT_RECORD_BYTES as usize {
                return Err(rquickjs::Exception::throw_range(
                    &ctx,
                    "log message exceeds protocol limits",
                ));
            }
            log_client
                .borrow_mut()
                .client
                .attempt_log(&message)
                .map_err(|error| js_error(&ctx, "log", error))
        }),
    )?;

    ctx.globals().set("yas", yas)?;
    ctx.eval::<(), _>(
        "globalThis.console = Object.freeze({\n\
         log: (...values) => yas.log(values.map(String).join(' ')),\n\
         error: (...values) => yas.log(values.map(String).join(' '))\n\
         }); Object.freeze(yas.context);",
    )?;
    Ok(())
}

fn wait_code(outcome: WaitOutcome) -> i32 {
    match outcome {
        WaitOutcome::Deadline => 0,
        WaitOutcome::Packet => 1,
        WaitOutcome::Closed => 2,
    }
}

fn js_error(ctx: &Ctx<'_>, operation: &str, error: impl fmt::Display) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, &format!("yas.{operation}: {error}"))
}

fn hex_hash(hash: &[u8; 32]) -> String {
    use fmt::Write as _;
    hash.iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn hex_bytes(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

fn lock_unpoison<T>(mutex: &std::sync::Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yas_wire::{
        Class, Decode, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
        core::{
            ClientHello, FamilyDescriptor, Operation, ReceiveLimits, ResultPrefix, RuntimeState,
            ServerHello, Status,
        },
        extension::{AttemptContext, AttemptOutput, OutputKind, Runtime as ExtensionRuntime},
        family,
    };

    const HASH: [u8; 32] = [0x2a; 32];

    fn spec(source: &str) -> AttemptSpec {
        AttemptSpec {
            source: Arc::from(source.as_bytes()),
            module_hash: HASH,
            extension_id: 7,
            label: Some("quickjs-test".into()),
            config: WasmiHostConfig::default(),
        }
    }

    fn selected_hello() -> ServerHello {
        ServerHello {
            minor: 1,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: ReceiveLimits::recommended(0),
            server_monotonic_ns: 3,
            catalog_revision: 1,
            server_name: "yas-test".into(),
            server_release: "1".into(),
            families: vec![
                FamilyDescriptor {
                    family_id: family::CORE,
                    version: yas_wire::core::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::TRANSFER,
                    version: yas_wire::transfer::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::CHANNEL,
                    version: yas_wire::channel::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: Vec::new(),
                    limits: yas_wire::channel::Limits::HARD.to_extensions().unwrap(),
                },
                FamilyDescriptor {
                    family_id: family::EXTENSION,
                    version: yas_wire::extension::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: vec![
                        Operation {
                            server_accepts: false,
                            server_sends: true,
                            class: Class::Event,
                            kind: yas_wire::extension::event_kind::ATTEMPT_CONTEXT,
                        },
                        Operation {
                            server_accepts: true,
                            server_sends: false,
                            class: Class::Event,
                            kind: yas_wire::extension::event_kind::ATTEMPT_OUTPUT,
                        },
                    ],
                    limits: yas_wire::extension::Limits::HARD.to_extensions().unwrap(),
                },
            ],
            extensions: Extensions::default(),
        }
    }

    fn attempt_context() -> AttemptContext {
        AttemptContext {
            extension_handle: 7,
            generation: 5,
            definition_revision: 3,
            attempt: 2,
            task_id: 11,
            flags: (yas_wire::schema::extension::DEFINITION_DETACHED
                | yas_wire::schema::extension::DEFINITION_PERSISTENT
                | yas_wire::schema::extension::DEFINITION_ENABLED
                | yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING)
                as u16,
            runtime: ExtensionRuntime::QuickJs,
            content_hash: HASH,
            name: "quickjs-test".into(),
            argv: vec![b"alpha".to_vec()],
            extensions: Extensions::default(),
        }
    }

    async fn take_packet(bridge: &HostBridge) -> Vec<u8> {
        let packet = bridge.recv_from_guest().await.unwrap();
        let bytes = packet.packet().to_vec();
        packet.acknowledge();
        bytes
    }

    async fn send_packet(bridge: &HostBridge, packet: Vec<u8>) {
        bridge
            .reserve_to_guest(packet.len())
            .await
            .unwrap()
            .commit(packet)
            .unwrap();
    }

    async fn boot(attempt: &mut QuickJsAttempt) -> HostBridge {
        attempt.wait_prepared().await.unwrap();
        let bridge = attempt.bridge();
        attempt.start().unwrap();
        assert_eq!(take_packet(&bridge).await, yas_wire::PREFACE);
        let pre_hello = FrameCodec::pre_hello();
        let hello_packet = take_packet(&bridge).await;
        let (request, consumed) = pre_hello.decode_stream(&hello_packet).unwrap();
        assert_eq!(consumed, hello_packet.len());
        assert_eq!(request.header.family, family::CORE);
        assert_eq!(request.header.kind, yas_wire::core::request_kind::HELLO);
        let offer = ClientHello::decode(&request.payload).unwrap();
        let hello = selected_hello();
        hello.validate_for_client(&offer).unwrap();
        let hello_result = ResultPrefix {
            status: Status::Ok,
            detail: Extensions::default(),
            body: hello.encode().unwrap(),
        };
        send_packet(
            &bridge,
            pre_hello
                .encode_stream(&Frame {
                    header: FrameHeader::result(
                        family::CORE,
                        yas_wire::core::request_kind::HELLO,
                        request.header.request_id.unwrap(),
                    ),
                    payload: hello_result.encode().unwrap(),
                })
                .unwrap(),
        )
        .await;
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        send_packet(
            &bridge,
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::event(
                            family::EXTENSION,
                            yas_wire::extension::event_kind::ATTEMPT_CONTEXT,
                        )
                    },
                    payload: attempt_context().encode().unwrap(),
                })
                .unwrap(),
        )
        .await;
        bridge
    }

    #[test]
    fn source_validation_rejects_syntax_and_non_utf8() {
        validate_source(b"export default () => 1", &WasmiHostConfig::default()).unwrap();
        let syntax = validate_source(b"export default (", &WasmiHostConfig::default()).unwrap_err();
        assert_eq!(syntax.kind, FailureKind::Validation);
        assert!(syntax.detail.contains("compile QuickJS source"));
        let utf8 = validate_source(&[0xff], &WasmiHostConfig::default()).unwrap_err();
        assert!(utf8.detail.contains("not UTF-8"));
    }

    #[test]
    fn native_bridge_inputs_reject_ambiguous_authority() {
        assert_eq!(parse_operation_id(&"01".repeat(16)).unwrap(), [1; 16]);
        assert!(parse_operation_id(&"00".repeat(16)).is_err());
        assert!(parse_operation_id(&"AA".repeat(16)).is_err());
        assert!(relative_path("../secret").is_err());
        assert!(relative_path("/absolute").is_err());
        assert_eq!(
            relative_path("one/two").unwrap().components,
            vec![b"one".to_vec(), b"two".to_vec()]
        );
    }

    #[tokio::test]
    async fn default_export_sees_native_context() {
        let mut attempt = spawn_attempt(spec(
            r#"
                export default function () {
                    if (yas.context.extensionHandle !== 7n) throw new Error("bad id");
                    if (yas.context.definitionRevision !== 3n) throw new Error("bad revision");
                    if (yas.context.argv[0] !== "alpha") throw new Error("bad args");
                    if (yas.context.bootId !== "01010101010101010101010101010101") throw new Error("bad boot");
                    return 9;
                }
            "#,
        ))
        .unwrap();
        boot(&mut attempt).await;
        assert_eq!(attempt.join().await.unwrap(), AttemptOutcome::Returned(9));
    }

    #[tokio::test]
    async fn top_level_only_module_returns_zero() {
        let mut attempt = spawn_attempt(spec("globalThis.quickjsRan = true;")).unwrap();
        boot(&mut attempt).await;
        assert_eq!(attempt.join().await.unwrap(), AttemptOutcome::Returned(0));
    }

    #[tokio::test]
    async fn native_extension_bridges_are_installed_and_bounded() {
        let mut attempt = spawn_attempt(spec(
            r#"
                export default function () {
                    for (const name of [
                        "commandReadStdin", "supports", "channelMessageLimit",
                        "environmentJson", "runProcessJson", "gitInspectJson",
                        "fsIndexFilesJson", "fsRead", "fsWrite", "blake3", "netExchange"
                    ]) {
                        if (typeof yas[name] !== "function") throw new Error(`missing ${name}`);
                    }
                    const digest = yas.blake3(new Uint8Array([1, 2, 3]));
                    if (!(digest instanceof Uint8Array) || digest.length !== 32) {
                        throw new Error("bad BLAKE3 result");
                    }
                    if (yas.channelMessageLimit() !== 16777216n) {
                        throw new Error("bad Channel limit");
                    }
                    if (yas.supports(66, 1, 2)) throw new Error("invented request support");
                    return 17;
                }
            "#,
        ))
        .unwrap();
        boot(&mut attempt).await;
        assert_eq!(attempt.join().await.unwrap(), AttemptOutcome::Returned(17));
    }

    #[tokio::test]
    async fn async_default_export_runs_jobs_and_returns_code() {
        let mut attempt = spawn_attempt(spec(
            "export default async function () { await Promise.resolve(); return 23; }",
        ))
        .unwrap();
        boot(&mut attempt).await;
        assert_eq!(attempt.join().await.unwrap(), AttemptOutcome::Returned(23));
    }

    #[tokio::test]
    async fn console_log_publishes_authenticated_attempt_output() {
        let mut attempt = spawn_attempt(spec(
            "export default function () { console.log('hello'); return 4; }",
        ))
        .unwrap();
        let bridge = boot(&mut attempt).await;
        let packet = take_packet(&bridge).await;
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        let (frame, consumed) = codec.decode_stream(&packet).unwrap();
        assert_eq!(consumed, packet.len());
        assert_eq!(
            frame.header,
            FrameHeader {
                sensitive: true,
                ..FrameHeader::event(
                    family::EXTENSION,
                    yas_wire::extension::event_kind::ATTEMPT_OUTPUT,
                )
            }
        );
        assert_eq!(
            AttemptOutput::decode(&frame.payload).unwrap(),
            AttemptOutput {
                kind: OutputKind::Log,
                data: b"hello".to_vec(),
                extensions: Extensions::default(),
            }
        );
        assert_eq!(attempt.join().await.unwrap(), AttemptOutcome::Returned(4));
    }

    #[tokio::test]
    async fn non_integer_return_is_a_trap() {
        let mut attempt =
            spawn_attempt(spec("export default function () { return 1.5; }")).unwrap();
        boot(&mut attempt).await;
        let AttemptOutcome::Failed(error) = attempt.join().await.unwrap() else {
            panic!("expected failed attempt");
        };
        assert_eq!(error.kind, FailureKind::Trap);
        assert!(error.detail.contains("must return an i32"));
    }

    #[tokio::test]
    async fn out_of_range_sleep_is_a_trap() {
        let mut attempt = spawn_attempt(spec(
            "export default function () { yas.sleep(Number.MAX_VALUE); }",
        ))
        .unwrap();
        boot(&mut attempt).await;
        let AttemptOutcome::Failed(error) = attempt.join().await.unwrap() else {
            panic!("expected failed attempt");
        };
        assert_eq!(error.kind, FailureKind::Trap);
        assert!(error.detail.contains("sleep duration is out of range"));
    }

    #[tokio::test]
    async fn thrown_exception_is_a_trap() {
        let mut attempt = spawn_attempt(spec(
            "export default function () { throw new Error('broken'); }",
        ))
        .unwrap();
        boot(&mut attempt).await;
        let AttemptOutcome::Failed(error) = attempt.join().await.unwrap() else {
            panic!("expected failed attempt");
        };
        assert_eq!(error.kind, FailureKind::Trap);
        assert!(error.detail.contains("broken"));
    }

    #[tokio::test]
    async fn interrupt_handler_cancels_compute_loop() {
        let mut attempt =
            spawn_attempt(spec("export default function () { while (true) {} }")).unwrap();
        boot(&mut attempt).await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        attempt.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(2), attempt.join())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome, AttemptOutcome::Cancelled);
    }
}
