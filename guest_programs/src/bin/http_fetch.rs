#[path = "../common.rs"]
mod common;

use std::collections::BTreeMap;
use url::Url;
use wasmos_guest_abi::{HttpRequest, HttpResponse};

fn main() -> Result<(), String> {
    unsafe {
        let req = common::encode(&HttpRequest {
            method: "GET".to_string(),
            url: Url::parse("https://example.com").unwrap(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        });
        let mut out = vec![0u8; 64 * 1024];
        let mut written = 0u32;
        let status = common::net_http(
            req.as_ptr() as i32,
            req.len() as i32,
            out.as_mut_ptr() as i32,
            out.len() as i32,
            &mut written as *mut u32 as i32,
        );
        if status != 0 {
            println!(
                "http-fetch request failed with {}",
                common::describe_status(status)
            );
            return Ok(());
        }
        let response: HttpResponse = common::decode(&out[..written as usize]);
        println!(
            "status={} body_prefix={}",
            response.status,
            String::from_utf8_lossy(&response.body[..response.body.len().min(120)])
        );
        Ok(())
    }
}
