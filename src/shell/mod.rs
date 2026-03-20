use crate::gui::GuiSubsystem;
use crate::network::NetworkSubsystem;
use crate::runtime::{AbiSelection, ProgramLaunchRequest};
use crate::scheduler::Scheduler;
use crate::vfs::VirtualFileSystem;
use anyhow::Result;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;

pub struct Shell {
    scheduler: Arc<Scheduler>,
    _vfs: Arc<RwLock<VirtualFileSystem>>,
    _network: Arc<NetworkSubsystem>,
    _gui: Arc<GuiSubsystem>,
}

impl Shell {
    pub fn new(
        scheduler: Arc<Scheduler>,
        vfs: Arc<RwLock<VirtualFileSystem>>,
        network: Arc<NetworkSubsystem>,
        gui: Arc<GuiSubsystem>,
    ) -> Self {
        Self {
            scheduler,
            _vfs: vfs,
            _network: network,
            _gui: gui,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();
        let mut stdout = io::stdout();

        loop {
            stdout.write_all(b"wasmos> ").await?;
            stdout.flush().await?;

            let Some(line) = lines.next_line().await? else {
                break;
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "exit" {
                break;
            }
            let response = self.handle_command(line).await?;
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn handle_command(&self, line: &str) -> Result<String> {
        let mut parts = line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "run" => {
                let module_path = parts.next().unwrap_or_default().to_string();
                let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                let task_id = self
                    .scheduler
                    .spawn(ProgramLaunchRequest {
                        name: module_path.clone(),
                        module_path,
                        args,
                        env: BTreeMap::new(),
                        abi: AbiSelection::default(),
                    })
                    .await?;
                self.scheduler.tick().await?;
                Ok(format!("spawned task {task_id}"))
            }
            "ps" => {
                let tasks = self.scheduler.list_tasks().await;
                Ok(format!("{:?}", tasks))
            }
            "sched" => Ok(format!("scheduler mode: {:?}", self.scheduler.mode())),
            unknown => Ok(format!("unknown command: {unknown}")),
        }
    }
}
