use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskState {
    Ready,
    Running,
    Waiting,
    Exited(i32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl TaskSpec {
    pub fn wasm(program: String) -> Self {
        Self { program, args: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskControlBlock {
    pub pid: u64,
    pub spec: TaskSpec,
    pub state: TaskState,
    pub ticks: u64,
}

#[derive(Default)]
pub struct Scheduler {
    next_pid: u64,
    ready: VecDeque<u64>,
    tasks: HashMap<u64, TaskControlBlock>,
}

impl Scheduler {
    pub fn enqueue(&mut self, spec: TaskSpec) -> u64 {
        self.next_pid += 1;
        let pid = self.next_pid;
        self.tasks.insert(pid, TaskControlBlock { pid, spec, state: TaskState::Ready, ticks: 0 });
        self.ready.push_back(pid);
        pid
    }

    pub fn tick(&mut self) -> Option<TaskControlBlock> {
        let pid = self.ready.pop_front()?;
        let task = self.tasks.get_mut(&pid)?;
        task.state = TaskState::Running;
        task.ticks += 1;
        let snapshot = task.clone();
        task.state = TaskState::Ready;
        self.ready.push_back(pid);
        Some(snapshot)
    }

    pub fn exit(&mut self, pid: u64, code: i32) {
        if let Some(task) = self.tasks.get_mut(&pid) {
            task.state = TaskState::Exited(code);
        }
    }
}
