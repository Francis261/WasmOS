mod gui;
mod host;
mod network;
mod os_error;
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
use tokio::time::{Duration, sleep};
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
    let mut shell = Shell::new(scheduler.clone(), vfs, network, gui);

    shell.run().await?;
    while scheduler.has_active_tasks().await {
        scheduler.run_ready_tasks(1).await?;
        sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
