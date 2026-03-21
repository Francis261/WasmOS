pub mod gui;
pub mod net;
pub mod scheduler;
pub mod shell;
pub mod vfs;
pub mod wasm;

use anyhow::Result;
use gui::GuiHost;
use net::NetworkController;
use scheduler::Scheduler;
use shell::Shell;
use std::sync::Arc;
use tokio::sync::Mutex;
use vfs::VirtualFileSystem;
use wasm::RuntimeHost;

pub struct WasmOs {
    pub vfs: Arc<VirtualFileSystem>,
    pub network: Arc<NetworkController>,
    pub gui: Arc<GuiHost>,
    pub scheduler: Arc<Mutex<Scheduler>>,
    pub runtime: Arc<RuntimeHost>,
    pub shell: Shell,
}

impl WasmOs {
    pub async fn bootstrap() -> Result<Self> {
        let vfs = Arc::new(VirtualFileSystem::new());
        let network = Arc::new(NetworkController::default());
        let gui = Arc::new(GuiHost::default());
        let scheduler = Arc::new(Mutex::new(Scheduler::default()));
        let runtime = Arc::new(RuntimeHost::new(vfs.clone(), network.clone(), gui.clone()));
        let shell = Shell::new(vfs.clone(), scheduler.clone(), runtime.clone());

        Ok(Self {
            vfs,
            network,
            gui,
            scheduler,
            runtime,
            shell,
        })
    }
}
