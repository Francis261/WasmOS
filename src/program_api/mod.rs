use crate::gui::{DrawCommand, GuiEvent, WindowDescriptor};
use crate::network::{HttpRequest, HttpResponse};
use crate::runtime::RuntimeContext;
use anyhow::Result;
use wasmtime::{Caller, Linker};

pub struct SystemCallRegistry;

impl SystemCallRegistry {
    pub fn link(linker: &mut Linker<RuntimeContext>) -> Result<()> {
        linker.func_wrap_async(
            "wasmos",
            "yield_now",
            |caller: Caller<'_, RuntimeContext>, ()| {
                Box::new(async move {
                    let _ = caller.data().task_id;
                    Ok(())
                })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_read",
            |_caller: Caller<'_, RuntimeContext>, (_path_ptr, _path_len): (i32, i32)| {
                Box::new(async move { Ok(0_i32) })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "vfs_write",
            |_caller: Caller<'_, RuntimeContext>,
             (_path_ptr, _path_len, _buf_ptr, _buf_len): (i32, i32, i32, i32)| {
                Box::new(async move { Ok(0_i32) })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "net_http",
            |_caller: Caller<'_, RuntimeContext>, (_req_ptr, _req_len): (i32, i32)| {
                Box::new(async move { Ok(0_i32) })
            },
        )?;

        linker.func_wrap_async(
            "wasmos",
            "gui_open_window",
            |_caller: Caller<'_, RuntimeContext>, (_desc_ptr, _desc_len): (i32, i32)| {
                Box::new(async move { Ok(1_i64) })
            },
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct GuestApi;

impl GuestApi {
    pub fn yield_now() {}
    pub fn file_read(_path: &str) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    pub fn file_write(_path: &str, _bytes: &[u8]) -> Result<()> {
        Ok(())
    }
    pub fn http_request(_request: HttpRequest) -> Result<HttpResponse> {
        Ok(HttpResponse {
            status: 501,
            headers: Default::default(),
            body: Vec::new(),
        })
    }
    pub fn create_window(_descriptor: WindowDescriptor) -> Result<u64> {
        Ok(0)
    }
    pub fn draw(_window_id: u64, _commands: &[DrawCommand]) -> Result<()> {
        Ok(())
    }
    pub fn poll_events(_window_id: u64) -> Result<Vec<GuiEvent>> {
        Ok(Vec::new())
    }
}
