use crate::runtime::{ProgramLaunchRequest, RuntimeBlockReason, RuntimePoll, WasmRuntime};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type TaskId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulingMode {
    Cooperative,
    Preemptive { quantum_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WaitState {
    Sleeping { wake_at_tick: u64 },
    Io { channel: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskState {
    Ready,
    Running { quantum_ms: u64 },
    Yielded,
    Waiting(WaitState),
    Exited(i32),
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskControlBlock {
    pub id: TaskId,
    pub name: String,
    pub module_path: String,
    pub state: TaskState,
    pub timeslices: u64,
    pub wake_tick: Option<u64>,
    pub wait_channel: Option<String>,
}

pub struct Scheduler {
    runtime: Arc<WasmRuntime>,
    mode: SchedulingMode,
    tasks: RwLock<BTreeMap<TaskId, TaskControlBlock>>,
    ready_queue: RwLock<VecDeque<TaskId>>,
    waiting_queue: RwLock<BTreeMap<TaskId, WaitState>>,
    clock_tick: Mutex<u64>,
}

impl Scheduler {
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        Self::new_with_mode(runtime, SchedulingMode::Cooperative)
    }

    pub fn new_with_mode(runtime: Arc<WasmRuntime>, mode: SchedulingMode) -> Self {
        Self {
            runtime,
            mode,
            tasks: RwLock::new(BTreeMap::new()),
            ready_queue: RwLock::new(VecDeque::new()),
            waiting_queue: RwLock::new(BTreeMap::new()),
            clock_tick: Mutex::new(0),
        }
    }

    pub async fn spawn(&self, request: ProgramLaunchRequest) -> Result<TaskId> {
        let task_id = self.runtime.allocate_task_id().await;
        let tcb = TaskControlBlock {
            id: task_id,
            name: request.name.clone(),
            module_path: request.module_path.clone(),
            state: TaskState::Ready,
            timeslices: 0,
            wake_tick: None,
            wait_channel: None,
        };
        self.tasks.write().await.insert(task_id, tcb);
        self.ready_queue.write().await.push_back(task_id);
        self.runtime.prepare_task(task_id, &request).await?;
        Ok(task_id)
    }

    pub async fn tick(&self) -> Result<()> {
        let current_tick = self.advance_clock().await;
        self.wake_sleeping_tasks(current_tick).await;

        let maybe_task = self.ready_queue.write().await.pop_front();
        if let Some(task_id) = maybe_task {
            self.execute_task(task_id, current_tick).await;
        }
        Ok(())
    }

    pub async fn run_task_once(&self, task_id: TaskId) -> Result<bool> {
        let current_tick = self.advance_clock().await;
        self.wake_sleeping_tasks(current_tick).await;

        let mut ready_queue = self.ready_queue.write().await;
        let Some(position) = ready_queue
            .iter()
            .position(|candidate| *candidate == task_id)
        else {
            return Ok(false);
        };
        ready_queue.remove(position);
        drop(ready_queue);

        self.execute_task(task_id, current_tick).await;
        Ok(true)
    }

    async fn execute_task(&self, task_id: TaskId, current_tick: u64) {
        let quantum_ms = self.quantum_ms();
        self.mark_running(task_id, quantum_ms).await;
        let outcome = self.runtime.resume(task_id, quantum_ms).await;
        match outcome {
            Ok(RuntimePoll::Ready) | Ok(RuntimePoll::Yielded) => {
                self.requeue_task(task_id, TaskState::Yielded).await;
            }
            Ok(RuntimePoll::Waiting(reason)) => {
                self.move_to_wait_queue(task_id, current_tick, reason).await;
            }
            Ok(RuntimePoll::Exited(exit_code)) => {
                self.mark_state(task_id, TaskState::Exited(exit_code)).await;
                self.waiting_queue.write().await.remove(&task_id);
            }
            Err(error) => {
                self.mark_state(task_id, TaskState::Failed(format_runtime_error(&error)))
                    .await;
                self.waiting_queue.write().await.remove(&task_id);
            }
        }
    }

    pub async fn run_ready_tasks(&self, rounds: usize) -> Result<()> {
        for _ in 0..rounds {
            if self.ready_queue.read().await.is_empty() {
                break;
            }
            self.tick().await?;
        }
        Ok(())
    }

    pub async fn notify_io(&self, channel: &str) {
        let mut waiting = self.waiting_queue.write().await;
        let ready: Vec<TaskId> = waiting
            .iter()
            .filter_map(|(task_id, state)| match state {
                WaitState::Io {
                    channel: task_channel,
                } if task_channel == channel => Some(*task_id),
                _ => None,
            })
            .collect();

        for task_id in ready {
            waiting.remove(&task_id);
            drop(waiting);
            self.requeue_task(task_id, TaskState::Ready).await;
            waiting = self.waiting_queue.write().await;
        }
    }

    pub async fn list_tasks(&self) -> Vec<TaskControlBlock> {
        self.tasks.read().await.values().cloned().collect()
    }

    pub async fn kill(&self, task_id: TaskId) -> bool {
        let exists = self.tasks.read().await.contains_key(&task_id);
        if !exists {
            return false;
        }
        self.waiting_queue.write().await.remove(&task_id);
        self.ready_queue
            .write()
            .await
            .retain(|candidate| *candidate != task_id);
        self.mark_state(task_id, TaskState::Failed("killed by shell".to_string()))
            .await;
        true
    }

    pub fn mode(&self) -> &SchedulingMode {
        &self.mode
    }

    pub async fn has_active_tasks(&self) -> bool {
        self.tasks
            .read()
            .await
            .values()
            .any(|task| !matches!(task.state, TaskState::Exited(_) | TaskState::Failed(_)))
    }

    pub async fn guest_stderr(&self, task_id: TaskId) -> Option<String> {
        self.runtime.guest_stderr(task_id).await
    }

    async fn advance_clock(&self) -> u64 {
        let mut tick = self.clock_tick.lock().await;
        *tick += 1;
        *tick
    }

    fn quantum_ms(&self) -> u64 {
        match self.mode {
            SchedulingMode::Cooperative => 0,
            SchedulingMode::Preemptive { quantum_ms } => quantum_ms,
        }
    }

    async fn wake_sleeping_tasks(&self, current_tick: u64) {
        let sleepers: Vec<TaskId> = {
            let waiting = self.waiting_queue.read().await;
            waiting
                .iter()
                .filter_map(|(task_id, state)| match state {
                    WaitState::Sleeping { wake_at_tick } if *wake_at_tick <= current_tick => {
                        Some(*task_id)
                    }
                    _ => None,
                })
                .collect()
        };

        for task_id in sleepers {
            self.waiting_queue.write().await.remove(&task_id);
            self.requeue_task(task_id, TaskState::Ready).await;
        }
    }

    async fn mark_running(&self, task_id: TaskId, quantum_ms: u64) {
        if let Some(task) = self.tasks.write().await.get_mut(&task_id) {
            task.state = TaskState::Running { quantum_ms };
            task.timeslices += 1;
            task.wake_tick = None;
            task.wait_channel = None;
        }
    }

    async fn move_to_wait_queue(
        &self,
        task_id: TaskId,
        current_tick: u64,
        reason: RuntimeBlockReason,
    ) {
        let wait_state = match reason {
            RuntimeBlockReason::Sleep { duration_ms } => WaitState::Sleeping {
                wake_at_tick: current_tick + duration_ms.max(1),
            },
            RuntimeBlockReason::Io { channel } => WaitState::Io { channel },
        };
        self.waiting_queue
            .write()
            .await
            .insert(task_id, wait_state.clone());
        self.mark_waiting(task_id, wait_state).await;
    }

    async fn mark_waiting(&self, task_id: TaskId, state: WaitState) {
        if let Some(task) = self.tasks.write().await.get_mut(&task_id) {
            task.state = TaskState::Waiting(state.clone());
            match state {
                WaitState::Sleeping { wake_at_tick } => {
                    task.wake_tick = Some(wake_at_tick);
                    task.wait_channel = None;
                }
                WaitState::Io { channel } => {
                    task.wake_tick = None;
                    task.wait_channel = Some(channel);
                }
            }
        }
    }

    async fn requeue_task(&self, task_id: TaskId, state: TaskState) {
        self.mark_state(task_id, state).await;
        self.ready_queue.write().await.push_back(task_id);
    }

    async fn mark_state(&self, task_id: TaskId, state: TaskState) {
        if let Some(task) = self.tasks.write().await.get_mut(&task_id) {
            task.state = state;
            if !matches!(task.state, TaskState::Waiting(_)) {
                task.wake_tick = None;
                task.wait_channel = None;
            }
        }
    }
}

fn format_runtime_error(error: &anyhow::Error) -> String {
    let display = format!("{error:#}");
    if display.contains('\n') {
        display
    } else {
        format!("{error:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::GuiSubsystem;
    use crate::host::HostBridge;
    use crate::network::NetworkSubsystem;
    use crate::runtime::{AbiSelection, AbiStrategy, ProgramLaunchRequest, WasiOptions};
    use crate::vfs::VirtualFileSystem;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_wasm(name: &str, wat_src: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift should not break tests")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}_{nonce}.wasm"));
        let wasm = wat::parse_str(wat_src).expect("wat should parse");
        fs::write(&path, wasm).expect("fixture should write");
        path.to_string_lossy().to_string()
    }

    fn scheduler_fixture(mode: SchedulingMode) -> Scheduler {
        let host = Arc::new(HostBridge::detect());
        let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
        let network = Arc::new(NetworkSubsystem::new(host.clone()));
        let gui = Arc::new(GuiSubsystem::new(host.clone()));
        let runtime =
            Arc::new(WasmRuntime::new(host, vfs, network, gui).expect("runtime should initialize"));
        Scheduler::new_with_mode(runtime, mode)
    }

    #[tokio::test]
    async fn spawn_then_run_exits_task() {
        let scheduler = scheduler_fixture(SchedulingMode::Cooperative);
        let module_path = write_wasm(
            "scheduler_exit",
            r#"(module (func (export "wasmos_resume") (result i32) i32.const 0))"#,
        );
        let task_id = scheduler
            .spawn(ProgramLaunchRequest {
                name: "exit_once".to_string(),
                module_path,
                args: vec![],
                env: BTreeMap::new(),
                abi: AbiSelection {
                    strategy: AbiStrategy::CustomOnly,
                    wasi: WasiOptions::default(),
                },
            })
            .await
            .expect("spawn should succeed");

        assert!(
            scheduler
                .run_task_once(task_id)
                .await
                .expect("run should succeed")
        );
        let task = scheduler
            .list_tasks()
            .await
            .into_iter()
            .find(|entry| entry.id == task_id)
            .expect("task must exist");
        assert!(
            matches!(task.state, TaskState::Exited(0)),
            "expected exit transition, got {:?}",
            task.state
        );
    }

    #[tokio::test]
    async fn sleep_syscall_moves_task_to_waiting_then_wakes() {
        let scheduler = scheduler_fixture(SchedulingMode::Cooperative);
        let module_path = write_wasm(
            "scheduler_sleep_wait",
            r#"(module
                (import "wasmos" "sleep_ms" (func $sleep (param i64) (result i32)))
                (global $slept (mut i32) (i32.const 0))
                (func (export "wasmos_resume") (result i32)
                    global.get $slept
                    i32.eqz
                    if
                        i64.const 2
                        call $sleep
                        drop
                        i32.const 1
                        global.set $slept
                    end
                    i32.const 0
                )
            )"#,
        );
        let task_id = scheduler
            .spawn(ProgramLaunchRequest {
                name: "sleeper".to_string(),
                module_path,
                args: vec![],
                env: BTreeMap::new(),
                abi: AbiSelection {
                    strategy: AbiStrategy::CustomOnly,
                    wasi: WasiOptions::default(),
                },
            })
            .await
            .expect("spawn should succeed");

        scheduler
            .run_task_once(task_id)
            .await
            .expect("run should succeed");
        let waiting = scheduler
            .list_tasks()
            .await
            .into_iter()
            .find(|entry| entry.id == task_id)
            .expect("task must exist after first run");
        assert!(
            matches!(
                waiting.state,
                TaskState::Waiting(WaitState::Sleeping { .. })
            ),
            "expected waiting/sleeping state, got {:?}",
            waiting.state
        );

        scheduler
            .tick()
            .await
            .expect("tick should advance scheduler");
        scheduler
            .tick()
            .await
            .expect("second tick should wake and run task");
        let final_state = scheduler
            .list_tasks()
            .await
            .into_iter()
            .find(|entry| entry.id == task_id)
            .expect("task must still be tracked");
        assert!(
            matches!(final_state.state, TaskState::Exited(0)),
            "expected exited state after wake, got {:?}",
            final_state.state
        );
    }

    #[test]
    fn runtime_error_formatter_keeps_context_chain() {
        let error = anyhow::anyhow!("inner cause").context("outer context");
        let formatted = format_runtime_error(&error);
        assert!(
            formatted.contains("outer context"),
            "formatted error should keep outer context: {formatted}"
        );
        assert!(
            formatted.contains("inner cause"),
            "formatted error should keep inner cause: {formatted}"
        );
    }
}
