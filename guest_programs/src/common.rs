#![allow(dead_code)]

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasmos_guest_abi::OsErrorCode;

#[link(wasm_import_module = "wasmos")]
unsafe extern "C" {
    pub fn vfs_open(req_ptr: i32, req_len: i32, fd_ptr: i32) -> i32;
    pub fn vfs_fd_read(req_ptr: i32, req_len: i32, out_ptr: i32, out_len: i32, bytes_written_ptr: i32) -> i32;
    pub fn vfs_fd_write(req_ptr: i32, req_len: i32, bytes_written_ptr: i32) -> i32;
    pub fn vfs_seek(req_ptr: i32, req_len: i32, pos_ptr: i32) -> i32;
    pub fn vfs_close(fd: u64) -> i32;
    pub fn vfs_list_dir(req_ptr: i32, req_len: i32, out_ptr: i32, out_len: i32, bytes_written_ptr: i32) -> i32;
    pub fn vfs_mkdir(req_ptr: i32, req_len: i32) -> i32;
    pub fn vfs_delete(req_ptr: i32, req_len: i32) -> i32;

    pub fn net_http(req_ptr: i32, req_len: i32, out_ptr: i32, out_len: i32, bytes_written_ptr: i32) -> i32;

    pub fn gui_open_window(desc_ptr: i32, desc_len: i32, window_id_ptr: i32) -> i32;
    pub fn gui_draw(req_ptr: i32, req_len: i32) -> i32;
    pub fn gui_poll_events(req_ptr: i32, req_len: i32, out_ptr: i32, out_len: i32, bytes_written_ptr: i32) -> i32;

    pub fn yield_now() -> i32;
    pub fn sleep_ms(duration_ms: i64) -> i32;
}

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("serialize")
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> T {
    serde_json::from_slice(bytes).expect("deserialize")
}

pub fn require_ok(code: i32, context: &str) {
    if code != 0 {
        panic!("{} failed with {}", context, describe_status(code));
    }
}

pub fn describe_status(code: i32) -> String {
    let status = OsErrorCode::from_i32(code)
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| "UnknownStatus".to_string());
    format!("errno {code} ({status})")
}
