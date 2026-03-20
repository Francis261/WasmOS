mod gui;
mod host;
mod network;
mod program_api;
mod runtime;
mod scheduler;
mod shell;
mod vfs;

use anyhow::Result;
use gui::GuiSubsystem;
use host::HostBridge;
use network::NetworkSubsystem;
use runtime::WasmRuntime;
use scheduler::Scheduler;
use shell::Shell;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;
use vfs::VirtualFileSystem;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let host = Arc::new(HostBridge::detect());
    let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
    let network = Arc::new(NetworkSubsystem::new(host.clone()));
    let gui = Arc::new(GuiSubsystem::new(host.clone()));
    let runtime = Arc::new(WasmRuntime::new(
        host.clone(),
        vfs.clone(),
        network.clone(),
        gui.clone(),
    )?);
    let scheduler = Arc::new(Scheduler::new(runtime));
    let mut shell = Shell::new(scheduler, vfs, network, gui);

    shell.run().await
}
