use crate::gui::GuiSubsystem;
use crate::host::HostBridge;
use crate::network::{NetworkSubsystem, SocketPolicy};
use crate::program_api::SystemCallRegistry;
use crate::scheduler::TaskId;
use crate::vfs::{TaskVfsPolicy, VirtualFileSystem};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use wasmtime::{Config, Engine, Instance, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::pipe::MemoryOutputPipe;
use wasmtime_wasi::preview1::WasiP1Ctx;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramLaunchRequest {
    pub name: String,
    pub module_path: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub abi: AbiSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiSelection {
    pub strategy: AbiStrategy,
    pub wasi: WasiOptions,
}

impl Default for AbiSelection {
    fn default() -> Self {
        Self {
            strategy: AbiStrategy::Hybrid,
            wasi: WasiOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AbiStrategy {
    PureWasi,
    CustomOnly,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasiOptions {
    pub inherit_stdio: bool,
    pub expose_cli_environment: bool,
}

impl Default for WasiOptions {
    fn default() -> Self {
        Self {
            inherit_stdio: true,
            expose_cli_environment: true,
        }
    }
}

pub struct RuntimeWasi {
    ctx: WasiP1Ctx,
    stderr: MemoryOutputPipe,
}

impl std::fmt::Debug for RuntimeWasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeWasi").finish_non_exhaustive()
    }
}

impl RuntimeWasi {
    fn build(request: &ProgramLaunchRequest) -> Result<Self> {
        let mut builder = WasiCtxBuilder::new();
        let stderr = MemoryOutputPipe::new(64 * 1024);
        if request.abi.wasi.inherit_stdio {
            builder.inherit_stdin();
            builder.inherit_stdout();
        }
        builder.stderr(stderr.clone());
        builder.args(&request.args);
        if request.abi.wasi.expose_cli_environment {
            for (key, value) in &request.env {
                builder.env(key, value);
            }
        }
        Ok(Self {
            ctx: builder.build_p1(),
            stderr,
        })
    }

    pub fn ctx(&mut self) -> &mut WasiP1Ctx {
        &mut self.ctx
    }

    pub fn stderr_contents(&self) -> String {
        String::from_utf8_lossy(&self.stderr.contents())
            .trim()
            .to_string()
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeBlockReason {
    Sleep { duration_ms: u64 },
    Io { channel: String },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum RuntimePoll {
    Ready,
    Yielded,
    Waiting(RuntimeBlockReason),
    Exited(i32),
}

#[derive(Debug, Default)]
pub struct TaskRuntimeControl {
    pub requested_yield: bool,
    pub block_reason: Option<RuntimeBlockReason>,
    pub last_quantum_ms: u64,
    pub resume_count: u64,
    pub fuel_limit: u64,
    pub fuel_remaining: u64,
    pub fuel_consumed_last_resume: u64,
    pub fuel_consumed_total: u64,
}

impl TaskRuntimeControl {
    pub fn reset_for_quantum(&mut self, quantum_ms: u64) {
        self.requested_yield = false;
        self.block_reason = None;
        self.last_quantum_ms = quantum_ms;
        self.resume_count += 1;
        self.fuel_limit = fuel_budget_for_quantum(quantum_ms);
        self.fuel_remaining = self.fuel_limit;
        self.fuel_consumed_last_resume = 0;
    }

    pub fn request_yield(&mut self) {
        self.requested_yield = true;
    }

    pub fn request_sleep(&mut self, duration_ms: u64) {
        self.block_reason = Some(RuntimeBlockReason::Sleep { duration_ms });
    }

    pub fn request_io_wait(&mut self, channel: String) {
        self.block_reason = Some(RuntimeBlockReason::Io { channel });
    }

    fn classify_completion(&self, exit_code: i32) -> RuntimePoll {
        if let Some(reason) = self.block_reason.clone() {
            RuntimePoll::Waiting(reason)
        } else if self.requested_yield {
            RuntimePoll::Yielded
        } else {
            RuntimePoll::Exited(exit_code)
        }
    }

    pub fn record_fuel(&mut self, remaining: u64) {
        self.fuel_remaining = remaining.min(self.fuel_limit);
        self.fuel_consumed_last_resume = self.fuel_limit.saturating_sub(self.fuel_remaining);
        self.fuel_consumed_total = self
            .fuel_consumed_total
            .saturating_add(self.fuel_consumed_last_resume);
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct RuntimeContext {
    pub task_id: TaskId,
    pub host: Arc<HostBridge>,
    pub vfs: Arc<RwLock<VirtualFileSystem>>,
    pub network: Arc<NetworkSubsystem>,
    pub gui: Arc<GuiSubsystem>,
    pub abi: AbiSelection,
    pub wasi: RuntimeWasi,
    pub control: Arc<Mutex<TaskRuntimeControl>>,
}

struct TaskInstance {
    state: Mutex<PersistentTaskState>,
}

struct PersistentTaskState {
    entry: TaskEntryPoint,
    store: Store<RuntimeContext>,
    instance: Instance,
    control: Arc<Mutex<TaskRuntimeControl>>,
}

#[derive(Debug, Clone, Copy)]
enum TaskEntryPoint {
    WasiStart,
    Main,
    Resume,
}

pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<RuntimeContext>,
    host: Arc<HostBridge>,
    vfs: Arc<RwLock<VirtualFileSystem>>,
    network: Arc<NetworkSubsystem>,
    gui: Arc<GuiSubsystem>,
    next_task_id: Mutex<TaskId>,
    tasks: RwLock<BTreeMap<TaskId, TaskInstance>>,
}

impl WasmRuntime {
    pub fn new(
        host: Arc<HostBridge>,
        vfs: Arc<RwLock<VirtualFileSystem>>,
        network: Arc<NetworkSubsystem>,
        gui: Arc<GuiSubsystem>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.async_support(true);
        config.consume_fuel(true);
        config.wasm_multi_memory(true);

        let engine = Engine::new(&config)?;
        let linker = Self::build_linker(&engine)?;

        Ok(Self {
            engine,
            linker,
            host,
            vfs,
            network,
            gui,
            next_task_id: Mutex::new(1),
            tasks: RwLock::new(BTreeMap::new()),
        })
    }

    fn build_linker(engine: &Engine) -> Result<Linker<RuntimeContext>> {
        let mut linker = Linker::new(engine);
        wasmtime_wasi::preview1::add_to_linker_async(
            &mut linker,
            |context: &mut RuntimeContext| context.wasi.ctx(),
        )?;
        SystemCallRegistry::link(&mut linker)?;
        Ok(linker)
    }

    pub async fn allocate_task_id(&self) -> TaskId {
        let mut guard = self.next_task_id.lock().await;
        let current = *guard;
        *guard += 1;
        current
    }

    pub async fn prepare_task(
        &self,
        task_id: TaskId,
        request: &ProgramLaunchRequest,
    ) -> Result<()> {
        let module = Module::from_file(&self.engine, &request.module_path)
            .with_context(|| format!("failed to load wasm module {}", request.module_path))?;
        self.validate_abi_strategy(&module, &request.abi)
            .with_context(|| format!("ABI strategy rejected module {}", request.module_path))?;
        self.network
            .set_policy(task_id, SocketPolicy::default())
            .await;
        self.vfs
            .write()
            .await
            .set_task_policy(task_id, TaskVfsPolicy::default());
        let control = Arc::new(Mutex::new(TaskRuntimeControl::default()));
        let context = RuntimeContext {
            task_id,
            host: self.host.clone(),
            vfs: self.vfs.clone(),
            network: self.network.clone(),
            gui: self.gui.clone(),
            abi: request.abi.clone(),
            wasi: RuntimeWasi::build(request)?,
            control: control.clone(),
        };
        let mut store = Store::new(&self.engine, context);
        store.set_fuel(10_000)?;
        let instance = self.linker.instantiate_async(&mut store, &module).await?;
        let entry = if instance
            .get_typed_func::<(), i32>(&mut store, "wasmos_resume")
            .is_ok()
        {
            TaskEntryPoint::Resume
        } else if instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .is_ok()
        {
            TaskEntryPoint::WasiStart
        } else if instance
            .get_typed_func::<(), i32>(&mut store, "main")
            .is_ok()
        {
            TaskEntryPoint::Main
        } else {
            TaskEntryPoint::Resume
        };

        self.tasks.write().await.insert(
            task_id,
            TaskInstance {
                state: Mutex::new(PersistentTaskState {
                    entry,
                    store,
                    instance,
                    control,
                }),
            },
        );
        Ok(())
    }

    pub async fn resume(&self, task_id: TaskId, quantum_ms: u64) -> Result<RuntimePoll> {
        let tasks = self.tasks.read().await;
        let task = tasks.get(&task_id).context("task not registered")?;
        let mut state = task.state.lock().await;
        let fuel_limit = fuel_budget_for_quantum(quantum_ms);
        {
            let mut control = state.control.lock().await;
            control.reset_for_quantum(quantum_ms);
        }
        state.store.set_fuel(fuel_limit)?;
        let instance = state.instance;
        let call_result = match state.entry {
            TaskEntryPoint::Resume => {
                let resume = instance
                    .get_typed_func::<(), i32>(&mut state.store, "wasmos_resume")
                    .context("task does not export required `wasmos_resume` function")?;
                resume.call_async(&mut state.store, ()).await
            }
            TaskEntryPoint::WasiStart => {
                let start = instance
                    .get_typed_func::<(), ()>(&mut state.store, "_start")
                    .context("task lost `_start` entrypoint")?;
                start.call_async(&mut state.store, ()).await.map(|_| 0)
            }
            TaskEntryPoint::Main => {
                let main = instance
                    .get_typed_func::<(), i32>(&mut state.store, "main")
                    .context("task lost `main` entrypoint")?;
                main.call_async(&mut state.store, ()).await
            }
        };
        let remaining_fuel = state.store.get_fuel().unwrap_or_default();
        {
            let mut control = state.control.lock().await;
            control.record_fuel(remaining_fuel);
            match call_result {
                Ok(exit_code) => return Ok(control.classify_completion(exit_code)),
                Err(error) if is_out_of_fuel_trap(&error) => {
                    control.request_yield();
                    return Ok(RuntimePoll::Yielded);
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn guest_stderr(&self, task_id: TaskId) -> Option<String> {
        let tasks = self.tasks.read().await;
        let task = tasks.get(&task_id)?;
        let state = task.state.lock().await;
        let stderr = state.store.data().wasi.stderr_contents();
        (!stderr.is_empty()).then_some(stderr)
    }

    fn validate_abi_strategy(&self, module: &Module, abi: &AbiSelection) -> Result<()> {
        let mut imports_wasi = false;
        let mut imports_custom = false;
        for import in module.imports() {
            match import.module() {
                "wasi_snapshot_preview1" => imports_wasi = true,
                "wasmos" => imports_custom = true,
                _ => {}
            }
        }

        match abi.strategy {
            AbiStrategy::PureWasi => {
                if imports_custom {
                    bail!("pure WASI mode does not allow `wasmos` imports")
                }
            }
            AbiStrategy::CustomOnly => {
                if imports_wasi {
                    bail!("custom-only mode does not allow `wasi_snapshot_preview1` imports")
                }
            }
            AbiStrategy::Hybrid => {
                if !imports_wasi && !imports_custom {
                    return Err(anyhow!(
                        "hybrid mode expects the guest to import either WASI or `wasmos` capabilities"
                    ));
                }
            }
        }

        let exports_start = module.exports().any(|export| export.name() == "_start");
        let exports_main = module.exports().any(|export| export.name() == "main");
        let exports_resume = module
            .exports()
            .any(|export| export.name() == "wasmos_resume");

        if matches!(abi.strategy, AbiStrategy::CustomOnly) && !exports_resume {
            bail!("custom-only guests must export `wasmos_resume` as the resumable entrypoint")
        }

        if !matches!(abi.strategy, AbiStrategy::CustomOnly) && !exports_start && !exports_main {
            bail!("WASI-backed guests must export `_start` or `main` as the process entrypoint")
        }

        if matches!(abi.strategy, AbiStrategy::Hybrid)
            && !exports_resume
            && !exports_start
            && !exports_main
        {
            bail!("hybrid guests must export `_start`, `main`, or `wasmos_resume`")
        }

        Ok(())
    }
}

fn fuel_budget_for_quantum(quantum_ms: u64) -> u64 {
    if quantum_ms == 0 {
        50_000
    } else {
        quantum_ms.saturating_mul(1_000).max(1_000)
    }
}

fn is_out_of_fuel_trap(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .to_string()
            .contains("all fuel consumed by WebAssembly")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::GuiSubsystem;
    use crate::host::HostBridge;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_path(name: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be monotonic enough for test naming")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("{name}_{nonce}.wasm"))
            .to_string_lossy()
            .to_string()
    }

    fn write_wasm(name: &str, wat_src: &str) -> String {
        let wasm_path = fixture_path(name);
        let wasm = wat::parse_str(wat_src).expect("wat fixture should parse");
        fs::write(&wasm_path, wasm).expect("fixture should write");
        wasm_path
    }

    fn runtime_fixture() -> WasmRuntime {
        let host = Arc::new(HostBridge::detect());
        let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
        let network = Arc::new(NetworkSubsystem::new(host.clone()));
        let gui = Arc::new(GuiSubsystem::new(host.clone()));
        WasmRuntime::new(host, vfs, network, gui).expect("runtime should initialize")
    }

    #[tokio::test]
    async fn validates_abi_custom_only_requires_resume_export() {
        let runtime = runtime_fixture();
        let module_path = write_wasm("abi_custom_missing_resume", "(module)");
        let err = runtime
            .prepare_task(
                1,
                &ProgramLaunchRequest {
                    name: "bad_custom".to_string(),
                    module_path,
                    args: vec![],
                    env: BTreeMap::new(),
                    abi: AbiSelection {
                        strategy: AbiStrategy::CustomOnly,
                        wasi: WasiOptions::default(),
                    },
                },
            )
            .await
            .expect_err("custom-only guest without wasmos_resume should fail");
        let details = format!("{err:#}");
        assert!(
            details.contains("custom-only guests must export `wasmos_resume`"),
            "unexpected error: {details}"
        );
    }

    #[tokio::test]
    async fn validates_abi_pure_wasi_rejects_custom_imports() {
        let runtime = runtime_fixture();
        let module_path = write_wasm(
            "abi_pure_wasi_imports_custom",
            r#"(module
                (import "wasmos" "yield_now" (func $yield (result i32)))
                (func (export "_start") call $yield drop)
            )"#,
        );
        let err = runtime
            .prepare_task(
                2,
                &ProgramLaunchRequest {
                    name: "bad_pure_wasi".to_string(),
                    module_path,
                    args: vec![],
                    env: BTreeMap::new(),
                    abi: AbiSelection {
                        strategy: AbiStrategy::PureWasi,
                        wasi: WasiOptions::default(),
                    },
                },
            )
            .await
            .expect_err("pure wasi guest with custom imports should fail");
        let details = format!("{err:#}");
        assert!(
            details.contains("pure WASI mode does not allow `wasmos` imports"),
            "unexpected error: {details}"
        );
    }

    #[tokio::test]
    async fn out_of_fuel_maps_to_runtime_yielded() {
        let runtime = runtime_fixture();
        let task_id = runtime.allocate_task_id().await;
        let module_path = write_wasm(
            "runtime_out_of_fuel",
            r#"(module
                (func (export "wasmos_resume") (result i32)
                    (loop br 0)
                    i32.const 0
                )
            )"#,
        );
        runtime
            .prepare_task(
                task_id,
                &ProgramLaunchRequest {
                    name: "busy_loop".to_string(),
                    module_path,
                    args: vec![],
                    env: BTreeMap::new(),
                    abi: AbiSelection {
                        strategy: AbiStrategy::CustomOnly,
                        wasi: WasiOptions::default(),
                    },
                },
            )
            .await
            .expect("fixture should prepare");

        let result = runtime
            .resume(task_id, 1)
            .await
            .expect("out-of-fuel should not be surfaced as hard failure");
        assert!(
            matches!(result, RuntimePoll::Yielded),
            "expected yielded from out-of-fuel preemption, got {result:?}"
        );
    }
}
