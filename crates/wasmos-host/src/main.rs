mod api;
mod gui;
mod net;
mod runtime;
mod scheduler;
mod shell;
mod vfs;

use anyhow::Result;
use api::build_router;
use gui::GuiBroker;
use net::NetManager;
use runtime::RuntimeHost;
use scheduler::Scheduler;
use shell::Shell;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::info;
use vfs::VirtualFileSystem;

#[derive(Clone)]
pub struct AppState {
    pub shell: Arc<Shell>,
    pub scheduler: Arc<Scheduler>,
    pub runtime: Arc<RuntimeHost>,
    pub vfs: Arc<VirtualFileSystem>,
    pub net: Arc<NetManager>,
    pub gui: Arc<GuiBroker>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let vfs = Arc::new(VirtualFileSystem::bootstrap());
    let gui = Arc::new(GuiBroker::default());
    let net = Arc::new(NetManager::default());
    let runtime = Arc::new(RuntimeHost::new(vfs.clone(), net.clone(), gui.clone())?);
    let scheduler = Arc::new(Scheduler::new(runtime.clone()));
    let shell = Arc::new(Shell::new(scheduler.clone(), vfs.clone()));

    let state = AppState {
        shell,
        scheduler,
        runtime,
        vfs,
        net,
        gui,
    };

    let app = build_router(state.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = TcpListener::bind(addr).await?;
    info!("starting WasmOS host on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
