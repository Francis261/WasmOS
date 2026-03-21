use crate::{
    scheduler::{Scheduler, SpawnRequest},
    vfs::VirtualFileSystem,
};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct Shell {
    scheduler: Arc<Scheduler>,
    vfs: Arc<VirtualFileSystem>,
}

#[derive(Clone, Serialize)]
pub struct ShellResponse {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl Shell {
    pub fn new(scheduler: Arc<Scheduler>, vfs: Arc<VirtualFileSystem>) -> Self {
        Self { scheduler, vfs }
    }

    pub async fn execute(&self, command: &str) -> ShellResponse {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        match tokens.as_slice() {
            ["help"] => ok(
                command,
                "commands: help, ls <path>, spawn <app> <program.wasm> [args...]".into(),
            ),
            ["ls", path] => match self.vfs.read_dir(path) {
                Ok(entries) => ok(
                    command,
                    entries
                        .into_iter()
                        .map(|entry| entry.path)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                Err(error) => err(command, error.to_string()),
            },
            ["spawn", app_id, program, rest @ ..] => match self
                .scheduler
                .spawn(SpawnRequest {
                    app_id: (*app_id).into(),
                    program: (*program).into(),
                    argv: rest.iter().map(|item| (*item).into()).collect(),
                })
                .await
            {
                Ok(task) => ok(command, format!("spawned {}", task.id)),
                Err(error) => err(command, error.to_string()),
            },
            [] => ok(command, String::new()),
            _ => err(command, "unknown command".into()),
        }
    }
}

fn ok(command: &str, stdout: String) -> ShellResponse {
    ShellResponse {
        command: command.into(),
        stdout,
        stderr: String::new(),
        status: 0,
    }
}

fn err(command: &str, stderr: String) -> ShellResponse {
    ShellResponse {
        command: command.into(),
        stdout: String::new(),
        stderr,
        status: 1,
    }
}
