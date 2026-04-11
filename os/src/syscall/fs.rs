use crate::fs::{make_pipe, open_file, OpenFlags, Stat, ROOT_INODE};
use crate::mm::{translated_byte_buffer, translated_refmut, translated_str, UserBuffer};
use crate::task::{current_process, current_user_token};
use alloc::sync::Arc;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let process = current_process();
    let inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() { return -1; }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() { return -1; }
        let file = file.clone();
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else { -1 }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let process = current_process();
    let inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() { return -1; }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() { return -1; }
        drop(inner);
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else { -1 }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let process = current_process();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = process.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else { -1 }
}

pub fn sys_close(fd: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() || inner.fd_table[fd].is_none() { return -1; }
    inner.fd_table[fd].take();
    0
}

pub fn sys_pipe(pipe: *mut usize) -> isize {
    let process = current_process();
    let token = current_user_token();
    let mut inner = process.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}

pub fn sys_dup(fd: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() || inner.fd_table[fd].is_none() { return -1; }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
}

pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let process = current_process(); 
    let inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() { return -1; }
    if let Some(_file) = &inner.fd_table[fd] {
        let token = inner.memory_set.token();
        let stat = Stat {
            dev: 0, ino: fd as u64,
            mode: crate::fs::StatMode::FILE,
            nlink: 1, pad: [0; 7],
        };
        drop(inner);
        *translated_refmut(token, st) = stat;
        0
    } else { -1 }
}

pub fn sys_linkat(old_name_ptr: *const u8, new_name_ptr: *const u8) -> isize {
    let token = current_user_token();
    let old_name = translated_str(token, old_name_ptr);
    let new_name = translated_str(token, new_name_ptr);
    if let Some(old_inode) = ROOT_INODE.find(old_name.as_str()) {
        if ROOT_INODE.find(new_name.as_str()).is_some() { return -1; }
        ROOT_INODE.link(new_name.as_str(), &old_inode)
    } else { -1 }
}

pub fn sys_unlinkat(name_ptr: *const u8) -> isize {
    let token = current_user_token();
    let name = translated_str(token, name_ptr);
    ROOT_INODE.unlink(name.as_str())
}