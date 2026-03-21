use crate::{
    gui::GuiBroker,
    net::NetManager,
    scheduler::{TaskRecord, TaskState},
    vfs::VirtualFileSystem,
};
use anyhow::{bail, Result};
use std::sync::Arc;
use wasmtime::{Config, Engine};

#[derive(Clone)]
pub struct RuntimeHost {
    engine: Engine,
    vfs: Arc<VirtualFileSystem>,
    net: Arc<NetManager>,
    gui: Arc<GuiBroker>,
}

#[derive(Clone)]
pub struct RuntimeTicket {
    pub state: TaskState,
}

impl RuntimeHost {
    pub fn new(
        vfs: Arc<VirtualFileSystem>,
        net: Arc<NetManager>,
        gui: Arc<GuiBroker>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.async_support(true);
        config.wasm_component_model(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config)?;
        Ok(Self {
            engine,
            vfs,
            net,
            gui,
        })
    }

    pub async fn launch(&self, task: TaskRecord) -> Result<RuntimeTicket> {
        if !task.program.ends_with(".wasm") {
            bail!("program must be a .wasm module");
        }
        let _engine = &self.engine;
        let _fs_caps = self.vfs.app_view(&task.app_id)?;
        let _net_caps = self.net.policy_for(&task.app_id);
        let _gui_session = self.gui.session_for(&task.app_id);
        Ok(RuntimeTicket {
            state: TaskState::Running,
        })
    }
}
