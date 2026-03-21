use crate::scheduler::TaskSpec;
use crate::vfs::VirtualFileSystem;
use crate::wasm::RuntimeHost;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::scheduler::Scheduler;

pub struct Shell {
    vfs: Arc<VirtualFileSystem>,
    scheduler: Arc<Mutex<Scheduler>>,
    runtime: Arc<RuntimeHost>,
}

impl Shell {
    pub fn new(vfs: Arc<VirtualFileSystem>, scheduler: Arc<Mutex<Scheduler>>, runtime: Arc<RuntimeHost>) -> Self {
        Self { vfs, scheduler, runtime }
    }

    pub async fn run_repl(&self) -> Result<()> {
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();
        let mut stdout = io::stdout();

        stdout.write_all(b"WasmOS shell ready\n").await?;
        while let Some(line) = lines.next_line().await? {
            let output = self.handle_command(&line).await?;
            stdout.write_all(output.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
        }
        Ok(())
    }

    pub async fn handle_command(&self, input: &str) -> Result<String> {
        let mut parts = input.split_whitespace();
        let Some(command) = parts.next() else { return Ok(String::new()) };
        match command {
            "help" => Ok("help | ls <path> | cat <path> | write <path> <content> | run <module>".to_string()),
            "ls" => {
                let entries = self.vfs.list(parts.next().unwrap_or("/"))?;
                Ok(entries.join(" "))
            }
            "cat" => {
                let path = parts.next().unwrap_or("/");
                Ok(self.vfs.read_to_string(path)?)
            }
            "write" => {
                let path = parts.next().unwrap_or("/tmp/out.txt");
                let content = parts.collect::<Vec<_>>().join(" ");
                self.vfs.write_string(path, &content)?;
                Ok(format!("wrote {}", path))
            }
            "run" => {
                let module = parts.next().unwrap_or("/bin/app.wasm");
                let spec = TaskSpec::wasm(module.to_string());
                self.scheduler.lock().await.enqueue(spec.clone());
                self.runtime.prepare_task(&spec).await?;
                Ok(format!("scheduled {}", module))
            }
            other => Ok(format!("unknown command: {}", other)),
        }
    }
}
