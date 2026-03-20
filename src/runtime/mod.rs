use crate::gui::GuiSubsystem;
use crate::network::{NetworkSubsystem, SocketPolicy};
use crate::program_api::SystemCallRegistry;
use crate::scheduler::TaskId;
use crate::vfs::VirtualFileSystem;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use wasmtime::{Config, Engine, Linker, Module, Store};

use crate::host::HostBridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramLaunchRequest {
    pub name: String,
    pub module_path: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub task_id: TaskId,
    pub host: Arc<HostBridge>,
    pub vfs: Arc<RwLock<VirtualFileSystem>>,
    pub network: Arc<NetworkSubsystem>,
    pub gui: Arc<GuiSubsystem>,
}

struct TaskInstance {
    request: ProgramLaunchRequest,
    module: Module,
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

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        SystemCallRegistry::link(&mut linker)?;

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
            },
        );
        Ok(())
    }

    pub async fn resume(&self, task_id: TaskId) -> Result<i32> {
        let tasks = self.tasks.read().await;
        let task = tasks.get(&task_id).context("task not registered")?;
        let context = RuntimeContext {
            task_id,
            host: self.host.clone(),
            vfs: self.vfs.clone(),
            network: self.network.clone(),
            gui: self.gui.clone(),
        };
        let mut store = Store::new(&self.engine, context);
        store.set_fuel(10_000)?;
        let instance = self
            .linker
            .instantiate_async(&mut store, &task.module)
            .await?;
        if let Ok(start) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
            start.call_async(&mut store, ()).await?;
        } else if let Ok(main) = instance.get_typed_func::<(), i32>(&mut store, "main") {
            return main.call_async(&mut store, ()).await.map_err(Into::into);
        }
        let _ = &task.request;
        Ok(0)
    }
}
