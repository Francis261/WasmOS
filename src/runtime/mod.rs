use crate::gui::GuiSubsystem;
use crate::host::HostBridge;
use crate::network::{NetworkSubsystem, SocketPolicy};
use crate::program_api::SystemCallRegistry;
use crate::scheduler::TaskId;
use crate::vfs::VirtualFileSystem;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use wasmtime::{Config, Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;
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
}

impl std::fmt::Debug for RuntimeWasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeWasi").finish_non_exhaustive()
    }
}

impl RuntimeWasi {
    fn build(request: &ProgramLaunchRequest) -> Result<Self> {
        let mut builder = WasiCtxBuilder::new();
        if request.abi.wasi.inherit_stdio {
            builder.inherit_stdio();
        }
        builder.args(&request.args);
        if request.abi.wasi.expose_cli_environment {
            for (key, value) in &request.env {
                builder.env(key, value);
            }
        }
        Ok(Self {
            ctx: builder.build_p1(),
        })
    }

    pub fn ctx(&mut self) -> &mut WasiP1Ctx {
        &mut self.ctx
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeBlockReason {
    Sleep { duration_ms: u64 },
    Io { channel: String },
}

#[derive(Debug, Clone)]
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
}

impl TaskRuntimeControl {
    pub fn reset_for_quantum(&mut self, quantum_ms: u64) {
        self.requested_yield = false;
        self.block_reason = None;
        self.last_quantum_ms = quantum_ms;
        self.resume_count += 1;
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
}

#[derive(Debug)]
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
    request: ProgramLaunchRequest,
    module: Module,
    control: Arc<Mutex<TaskRuntimeControl>>,
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
            .set_policy(
                task_id,
                SocketPolicy {
                    allow_remote: false,
                    allowed_hosts: Vec::new(),
                },
            )
            .await;
        self.tasks.write().await.insert(
            task_id,
            TaskInstance {
                request: request.clone(),
                module,
                control: Arc::new(Mutex::new(TaskRuntimeControl::default())),
            },
        );
        Ok(())
    }

    pub async fn resume(&self, task_id: TaskId, quantum_ms: u64) -> Result<RuntimePoll> {
        let tasks = self.tasks.read().await;
        let task = tasks.get(&task_id).context("task not registered")?;
        {
            let mut control = task.control.lock().await;
            control.reset_for_quantum(quantum_ms);
        }
        let context = RuntimeContext {
            task_id,
            host: self.host.clone(),
            vfs: self.vfs.clone(),
            network: self.network.clone(),
            gui: self.gui.clone(),
            abi: task.request.abi.clone(),
            wasi: RuntimeWasi::build(&task.request)?,
            control: task.control.clone(),
        };
        let mut store = Store::new(&self.engine, context);
        store.set_fuel(10_000)?;
        let instance = self
            .linker
            .instantiate_async(&mut store, &task.module)
            .await?;
        let exit_code = if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
            start.call_async(&mut store, ()).await?;
            0
        } else if let Ok(main) = instance.get_typed_func::<(), i32>(&mut store, "main") {
            main.call_async(&mut store, ()).await?
        } else {
            0
        };
        let control = task.control.lock().await;
        Ok(control.classify_completion(exit_code))
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

        if !matches!(abi.strategy, AbiStrategy::CustomOnly)
            && !module.exports().any(|export| export.name() == "_start")
        {
            bail!("WASI-backed guests must export `_start` as the process entrypoint")
        }

        Ok(())
    }
}
