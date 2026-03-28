//! Process management syscalls

use crate::mm::translated_byte_buffer;
use crate::task::current_user_token;

use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
// pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
//     trace!("kernel: sys_get_time");
//     -1
// }

 

pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = crate::timer::get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    
    // 获取当前进程的页表 token
    let token = current_user_token();
    
    // 将结构体转换为字节切片
    let time_bytes = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const _ as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    
    // 使用页表翻译并获取物理内存缓冲区的切片列表 (防止跨页)
    let buffers = translated_byte_buffer(token, ts as *const u8, core::mem::size_of::<TimeVal>());
    
    // 逐个分发写入到对应的物理页中
    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&time_bytes[offset..offset + len]);
        offset += len;
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
// pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
//     trace!("kernel: sys_trace");
//     -1
// }
 
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        // 请求 0：读取内存（将虚拟地址 id 翻译为物理地址后读取 1 字节）
        0 => {
            let token = current_user_token();
            let buffers = translated_byte_buffer(token, id as *const u8, 1);
            buffers[0][0] as isize
        },
        // 请求 1：写入内存（将虚拟地址 id 翻译为物理地址后写入 1 字节）
        1 => {
            let token = current_user_token();
            let mut buffers = translated_byte_buffer(token, id as *const u8, 1);
            buffers[0][0] = data as u8;
            0
        },
        // 请求 2：获取系统调用计数
        2 => {
            crate::task::get_current_syscall_times(id) as isize
        },
        _ => -1,
    }
}




// // YOUR JOB: Implement mmap.
// pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
//     trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
//     -1
// }

// // YOUR JOB: Implement munmap.
// pub fn sys_munmap(_start: usize, _len: usize) -> isize {
//     trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
//     -1
// }

///
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    crate::task::mmap_current(start, len, port)
}
///
pub fn sys_munmap(start: usize, len: usize) -> isize {
    crate::task::munmap_current(start, len)
}


/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
