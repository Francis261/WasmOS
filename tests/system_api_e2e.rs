use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use wasmos::gui::{DrawCommand, GuiSubsystem, WindowDescriptor};
use wasmos::host::HostBridge;
use wasmos::network::NetworkSubsystem;
use wasmos::runtime::{AbiSelection, AbiStrategy, ProgramLaunchRequest, WasiOptions, WasmRuntime};
use wasmos::scheduler::{Scheduler, SchedulingMode, TaskState};
use wasmos::vfs::VirtualFileSystem;

fn write_wasm(name: &str, wat_src: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic enough for fixture naming")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{name}_{nonce}.wasm"));
    let wasm = wat::parse_str(wat_src).expect("wat should parse");
    fs::write(&path, wasm).expect("fixture should write");
    path.to_string_lossy().to_string()
}

#[tokio::test]
async fn gui_window_lifecycle_is_host_backed() {
    let host = Arc::new(HostBridge::detect());
    let gui = GuiSubsystem::new(host);
    let window_id = gui
        .create_window(
            99,
            WindowDescriptor {
                title: "test-window".to_string(),
                width: 160,
                height: 120,
            },
        )
        .await;
    gui.draw(
        99,
        window_id,
        vec![
            DrawCommand::Clear {
                rgba: [0, 0, 0, 255],
            },
            DrawCommand::Text {
                x: 2,
                y: 2,
                text: "ok".to_string(),
                rgba: [255, 255, 255, 255],
            },
        ],
    )
    .await;
    let events = gui.poll_events(99, window_id).await;
    let windows = gui.list_windows().await;

    assert!(
        events.is_empty(),
        "in-memory backend starts with empty event queue"
    );
    assert_eq!(windows.len(), 1, "window should be tracked by subsystem");
    assert_eq!(windows[0].id, window_id);
    assert_eq!(windows[0].owner_task_id, 99);
}

#[tokio::test]
async fn yield_syscall_roundtrip_sets_scheduler_yield_state() {
    let host = Arc::new(HostBridge::detect());
    let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
    let network = Arc::new(NetworkSubsystem::new(host.clone()));
    let gui = Arc::new(GuiSubsystem::new(host.clone()));
    let runtime =
        Arc::new(WasmRuntime::new(host, vfs, network, gui).expect("runtime should initialize"));
    let scheduler = Scheduler::new_with_mode(runtime, SchedulingMode::Cooperative);

    let module_path = write_wasm(
        "yield_syscall_roundtrip",
        r#"(module
            (import "wasmos" "yield_now" (func $yield_now (result i32)))
            (func (export "wasmos_resume") (result i32)
                call $yield_now
                drop
                i32.const 0
            )
        )"#,
    );
    let task_id = scheduler
        .spawn(ProgramLaunchRequest {
            name: "yielding_guest".to_string(),
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
        .expect("single run should succeed");
    let task = scheduler
        .list_tasks()
        .await
        .into_iter()
        .find(|entry| entry.id == task_id)
        .expect("task should exist");
    assert!(
        matches!(task.state, TaskState::Yielded | TaskState::Ready),
        "yield syscall should keep task runnable, got {:?}",
        task.state
    );
}
