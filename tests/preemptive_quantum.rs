use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use tokio::sync::RwLock;
use wasmos::gui::GuiSubsystem;
use wasmos::host::HostBridge;
use wasmos::network::NetworkSubsystem;
use wasmos::runtime::{AbiSelection, AbiStrategy, ProgramLaunchRequest, WasiOptions, WasmRuntime};
use wasmos::scheduler::{Scheduler, SchedulingMode, TaskState};
use wasmos::vfs::VirtualFileSystem;

#[tokio::test]
async fn preemptive_scheduler_timeslices_non_yielding_guest() {
    let wasm_path =
        write_non_yielding_resume_module().expect("non-yielding fixture should compile");

    let host = Arc::new(HostBridge::detect());
    let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
    let network = Arc::new(NetworkSubsystem::new(host.clone()));
    let gui = Arc::new(GuiSubsystem::new(host.clone()));
    let runtime = Arc::new(
        WasmRuntime::new(host.clone(), vfs.clone(), network.clone(), gui.clone())
            .expect("runtime should initialize"),
    );
    let scheduler = Scheduler::new_with_mode(runtime, SchedulingMode::Preemptive { quantum_ms: 1 });

    let task_id = scheduler
        .spawn(ProgramLaunchRequest {
            name: "non_yielding".to_string(),
            module_path: wasm_path,
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
        .run_ready_tasks(4)
        .await
        .expect("preemptive run loop should not fail");

    let tasks = scheduler.list_tasks().await;
    let task = tasks
        .into_iter()
        .find(|entry| entry.id == task_id)
        .expect("task should be listed");

    assert!(
        task.timeslices >= 4,
        "expected multiple preemptive slices, got {}; state={:?}",
        task.timeslices,
        task.state
    );
    assert!(
        matches!(
            task.state,
            TaskState::Yielded | TaskState::Ready | TaskState::Running { .. }
        ),
        "expected task to remain runnable after out-of-fuel preemption, got {:?}",
        task.state
    );
}

fn write_non_yielding_resume_module() -> anyhow::Result<String> {
    let wasm_path =
        std::env::temp_dir().join(format!("non_yielding_resume_{}.wasm", std::process::id()));
    let wasm = wat::parse_str(
        r#"(module
            (func (export "wasmos_resume") (result i32)
                (loop
                    br 0
                )
                i32.const 0
            )
        )"#,
    )?;
    fs::write(&wasm_path, wasm)?;
    Ok(wasm_path.to_string_lossy().to_string())
}
