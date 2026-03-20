use crate::runtime::{ProgramLaunchRequest, WasmRuntime};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

pub type TaskId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulingMode {
    Cooperative,
    Preemptive { quantum_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskState {
    Ready,
    Running,
    Waiting,
    Exited(i32),
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskControlBlock {
    pub id: TaskId,
    pub name: String,
    pub module_path: String,
    pub state: TaskState,
}

pub struct Scheduler {
    runtime: Arc<WasmRuntime>,
    mode: SchedulingMode,
    tasks: RwLock<BTreeMap<TaskId, TaskControlBlock>>,
    run_queue: RwLock<VecDeque<TaskId>>,
}

impl Scheduler {
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        Self {
            runtime,
            mode: SchedulingMode::Cooperative,
            tasks: RwLock::new(BTreeMap::new()),
            run_queue: RwLock::new(VecDeque::new()),
        }
    }

    pub async fn spawn(&self, request: ProgramLaunchRequest) -> Result<TaskId> {
        let task_id = self.runtime.allocate_task_id().await;
        let tcb = TaskControlBlock {
            id: task_id,
            name: request.name.clone(),
            module_path: request.module_path.clone(),
            state: TaskState::Ready,
        };
        self.tasks.write().await.insert(task_id, tcb);
        self.run_queue.write().await.push_back(task_id);
        self.runtime.prepare_task(task_id, &request).await?;
        Ok(task_id)
    }

    pub async fn tick(&self) -> Result<()> {
        let maybe_task = self.run_queue.write().await.pop_front();
        if let Some(task_id) = maybe_task {
            self.mark_state(task_id, TaskState::Running).await;
            let outcome = self.runtime.resume(task_id).await;
            match outcome {
                Ok(exit_code) => self.mark_state(task_id, TaskState::Exited(exit_code)).await,
                Err(error) => {
                    self.mark_state(task_id, TaskState::Failed(error.to_string()))
                        .await
                }
            }
        }
        Ok(())
    }

    pub async fn list_tasks(&self) -> Vec<TaskControlBlock> {
        self.tasks.read().await.values().cloned().collect()
    }

    pub fn mode(&self) -> &SchedulingMode {
        &self.mode
    }

    async fn mark_state(&self, task_id: TaskId, state: TaskState) {
        if let Some(task) = self.tasks.write().await.get_mut(&task_id) {
            task.state = state;
        }
    }
}
