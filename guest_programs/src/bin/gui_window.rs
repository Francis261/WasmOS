#[path = "../common.rs"]
mod common;

use wasmos_guest_abi::{DrawCommand, GuiDrawRequest, WindowDescriptor};

fn main() {
    unsafe {
        let desc = common::encode(&WindowDescriptor {
            title: "WasmOS Guest Window".to_string(),
            width: 320,
            height: 180,
        });
        let mut window_id = 0u64;
        common::require_ok(
            common::gui_open_window(
                desc.as_ptr() as i32,
                desc.len() as i32,
                &mut window_id as *mut u64 as i32,
            ),
            "gui_open_window",
        );

        let draw = common::encode(&GuiDrawRequest {
            window_id,
            commands: vec![
                DrawCommand::Clear { rgba: [10, 20, 40, 255] },
                DrawCommand::Text {
                    x: 12,
                    y: 16,
                    text: "Hello from guest GUI".to_string(),
                    rgba: [240, 240, 240, 255],
                },
            ],
        });
        common::require_ok(common::gui_draw(draw.as_ptr() as i32, draw.len() as i32), "gui_draw");
        println!("opened and drew window {window_id}");
    }
}
