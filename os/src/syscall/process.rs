//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// // TODO: implement the syscall
// pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
//     trace!("kernel: sys_trace");
//     -1
// }

/// 实现任务系统调用跟踪

/// 实现任务系统调用跟踪与非安全读写
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        // 请求 0：读取内存
        0 => unsafe {
            let ptr = id as *const u8;
            *ptr as isize
        },
        // 请求 1：写入内存
        1 => unsafe {
            let ptr = id as *mut u8;
            *ptr = data as u8;
            0 // 写入成功返回 0
        },
        // 请求 2：获取系统调用计数
        2 => {
            crate::task::get_current_syscall_times(id) as isize
        },
        // 其他请求：返回错误码 -1
        _ => -1,
    }
}
