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
    fmt,
    rc::Rc,
    sync::{Arc, MutexGuard, atomic::Ordering},
    thread,
    time::Duration,
};
use tokio::sync::oneshot;
use yas_guest::{
    Client, MonotonicInstant, WaitOutcome,
    command::{CommandProvider, Invocation, ProviderEvent},
    native_host,
};

const SOURCE_NAME: &str = "extension.js";
const RANDOM_MAX_BYTES: usize = 16 * 1024 * 1024;

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
    ) -> Result<Option<yas_guest::command::InvocationRequest>, yas_guest::command::Error> {
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
                self.invocation = Some(*invocation);
                Ok(Some(request))
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
                .map(|request| {
                    let value = Object::new(ctx.clone())?;
                    let args = Array::new(ctx.clone())?;
                    for (index, argument) in request.args.iter().enumerate() {
                        args.set(index, argument.as_str())?;
                    }
                    value.set("args", args)?;
                    value.set("streamsStdin", request.streams_stdin)?;
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
