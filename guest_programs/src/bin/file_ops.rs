#[path = "../common.rs"]
mod common;

use wasmos_guest_abi::{
    OPEN_CREATE, OPEN_READ, OPEN_TRUNCATE, OPEN_WRITE, SeekWhence, VfsDirEntry, VfsOpenRequest,
    VfsPathRequest, VfsReadRequest, VfsSeekRequest, VfsWriteRequest,
};

fn main() {
    unsafe {
        let req = common::encode(&VfsPathRequest {
            path: "/tmp".to_string(),
        });
        common::require_ok(common::vfs_mkdir(req.as_ptr() as i32, req.len() as i32), "mkdir");

        let open_req = common::encode(&VfsOpenRequest {
            path: "/tmp/hello.txt".to_string(),
            flags: OPEN_WRITE | OPEN_CREATE | OPEN_TRUNCATE,
        });
        let mut fd = 0u64;
        common::require_ok(
            common::vfs_open(open_req.as_ptr() as i32, open_req.len() as i32, &mut fd as *mut u64 as i32),
            "open write",
        );

        let write_req = common::encode(&VfsWriteRequest {
            fd,
            data: b"Hello from guest file_ops".to_vec(),
        });
        let mut bytes_written = 0u32;
        common::require_ok(
            common::vfs_fd_write(
                write_req.as_ptr() as i32,
                write_req.len() as i32,
                &mut bytes_written as *mut u32 as i32,
            ),
            "write",
        );
        common::require_ok(common::vfs_close(fd), "close write fd");

        let open_read_req = common::encode(&VfsOpenRequest {
            path: "/tmp/hello.txt".to_string(),
            flags: OPEN_READ,
        });
        let mut read_fd = 0u64;
        common::require_ok(
            common::vfs_open(
                open_read_req.as_ptr() as i32,
                open_read_req.len() as i32,
                &mut read_fd as *mut u64 as i32,
            ),
            "open read",
        );

        let seek_req = common::encode(&VfsSeekRequest {
            fd: read_fd,
            offset: 0,
            whence: SeekWhence::Start,
        });
        let mut position = 0u64;
        common::require_ok(
            common::vfs_seek(
                seek_req.as_ptr() as i32,
                seek_req.len() as i32,
                &mut position as *mut u64 as i32,
            ),
            "seek",
        );

        let read_req = common::encode(&VfsReadRequest { fd: read_fd, len: 128 });
        let mut buf = vec![0u8; 128];
        let mut bytes_read = 0u32;
        common::require_ok(
            common::vfs_fd_read(
                read_req.as_ptr() as i32,
                read_req.len() as i32,
                buf.as_mut_ptr() as i32,
                buf.len() as i32,
                &mut bytes_read as *mut u32 as i32,
            ),
            "read",
        );
        common::require_ok(common::vfs_close(read_fd), "close read fd");

        println!(
            "read {} bytes: {}",
            bytes_read,
            String::from_utf8_lossy(&buf[..bytes_read as usize])
        );

        let list_req = common::encode(&VfsPathRequest {
            path: "/tmp".to_string(),
        });
        let mut list_buf = vec![0u8; 4096];
        let mut list_written = 0u32;
        common::require_ok(
            common::vfs_list_dir(
                list_req.as_ptr() as i32,
                list_req.len() as i32,
                list_buf.as_mut_ptr() as i32,
                list_buf.len() as i32,
                &mut list_written as *mut u32 as i32,
            ),
            "list",
        );
        let entries: Vec<VfsDirEntry> = common::decode(&list_buf[..list_written as usize]);
        println!("entries in /tmp: {:?}", entries.iter().map(|e| (&e.path, e.len)).collect::<Vec<_>>());

        let delete_req = common::encode(&VfsPathRequest {
            path: "/tmp/hello.txt".to_string(),
        });
        common::require_ok(
            common::vfs_delete(delete_req.as_ptr() as i32, delete_req.len() as i32),
            "delete",
        );
    }
}
