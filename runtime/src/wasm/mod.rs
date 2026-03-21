use crate::gui::{GuiCommand, GuiHost};
use crate::net::NetworkController;
use crate::scheduler::TaskSpec;
use crate::vfs::VirtualFileSystem;
use anyhow::Result;
use std::sync::Arc;
use wasmtime::{Config, Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;

pub struct RuntimeHost {
    engine: Engine,
    linker: Linker<wasmtime_wasi::WasiCtx>,
    vfs: Arc<VirtualFileSystem>,
    network: Arc<NetworkController>,
    gui: Arc<GuiHost>,
}

impl RuntimeHost {
    pub fn new(vfs: Arc<VirtualFileSystem>, network: Arc<NetworkController>, gui: Arc<GuiHost>) -> Self {
        let mut config = Config::new();
        config.wasm_component_model(true);
        let engine = Engine::new(&config).expect("engine");
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker(&mut linker, |ctx| ctx).expect("wasi linker");
        Self { engine, linker, vfs, network, gui }
    }

    pub async fn prepare_task(&self, spec: &TaskSpec) -> Result<()> {
        let _ = &self.vfs;
        let _ = &self.network;
        let _ = self.gui.apply(GuiCommand::CreateWindow(crate::gui::WindowSpec {
            title: format!("{}", spec.program),
            width: 800,
            height: 600,
        }));
        Ok(())
    }

    pub fn load_module(&self, bytes: &[u8]) -> Result<Module> {
        Module::from_binary(&self.engine, bytes).map_err(Into::into)
    }

    pub fn create_store(&self) -> Store<wasmtime_wasi::WasiCtx> {
        let wasi = WasiCtxBuilder::new().inherit_stdio().build();
        Store::new(&self.engine, wasi)
    }

    pub fn linker(&self) -> &Linker<wasmtime_wasi::WasiCtx> {
        &self.linker
    }
}
