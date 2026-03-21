use crate::runtime::{RuntimeHost, RuntimeTicket};
use anyhow::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

#[derive(Clone)]
pub struct Scheduler {
    runtime: Arc<RuntimeHost>,
    tasks: Arc<RwLock<BTreeMap<Uuid, TaskRecord>>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
    pub program: String,
    pub argv: Vec<String>,
    pub app_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: Uuid,
    pub state: TaskState,
    pub program: String,
    pub argv: Vec<String>,
    pub app_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Ready,
    Running,
    Yielded,
    Exited,
    Faulted,
}

impl Scheduler {
    pub fn new(runtime: Arc<RuntimeHost>) -> Self {
        Self {
            runtime,
            tasks: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub async fn spawn(&self, req: SpawnRequest) -> Result<TaskRecord> {
        let task = TaskRecord {
            id: Uuid::new_v4(),
            state: TaskState::Ready,
            program: req.program.clone(),
            argv: req.argv.clone(),
            app_id: req.app_id.clone(),
        };
        self.tasks.write().insert(task.id, task.clone());
        let ticket: RuntimeTicket = self.runtime.launch(task.clone()).await?;
        self.tasks
            .write()
            .entry(task.id)
            .and_modify(|record| record.state = ticket.state);
        Ok(self
            .tasks
            .read()
            .get(&task.id)
            .cloned()
            .expect("task inserted"))
    }

    pub fn snapshot(&self) -> Vec<serde_json::Value> {
        self.tasks
            .read()
            .values()
            .map(|task| serde_json::json!(task))
            .collect()
    }
}
