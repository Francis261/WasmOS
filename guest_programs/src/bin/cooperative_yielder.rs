#[path = "../common.rs"]
mod common;

static mut ITERATION: u32 = 0;

#[unsafe(no_mangle)]
pub extern "C" fn wasmos_resume() -> i32 {
    unsafe {
        if ITERATION >= 5 {
            return 0;
        }
        let iteration = ITERATION;
        println!("cooperative resume iteration {}", iteration);
        ITERATION += 1;
        let _ = common::yield_now();
        let _ = common::sleep_ms(1);
        0
    }
}

fn main() {}
