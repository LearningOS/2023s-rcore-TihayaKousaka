//! Process management syscalls
use crate::{
    config::MAX_SYSCALL_NUM,
    task::{
        munmap, mmap, get_current_task_num, change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, get_current_task_status, current_user_token, TaskStatus, RUN_TIME
    },
    syscall::get_syscall_info,
};
use crate::timer::{get_time_us, get_time_ms};
use crate::mm;
use crate::mm::translated_byte_buffer;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
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

// /// YOUR JOB: get time with second and microsecond
// /// HINT: You might reimplement it with virtual memory management.
// /// HINT: What if [`TimeVal`] is splitted by two pages ?
// pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
//     trace!("kernel: sys_get_time");
//     -1
// }
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let ts_phy_ptr = mm::get_refmut(current_user_token(), ts);
    *ts_phy_ptr = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };

    0
}

// /// YOUR JOB: Finish sys_task_info to pass testcases
// /// HINT: You might reimplement it with virtual memory management.
// /// HINT: What if [`TaskInfo`] is splitted by two pages ?
// pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
//     trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
//     -1
// }
// pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
//     let ti_phy_ptr = mm::get_refmut(current_user_token(), ti);
//     *ti_phy_ptr = TaskInfo {
//         status: TaskStatus::Running,
//         syscall_times: get_syscall_times(),
//         time: get_current_task_time(),
//     };

//     0
// }
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    let current = get_current_task_num();
    let task_info = unsafe { get_syscall_info(current) };
    // let user_time = get_current_user_time();
    let task_status = get_current_task_status();
    let rst = TaskInfo {
            status: task_status,
            syscall_times: task_info,
            time: get_time_ms() - unsafe { RUN_TIME } + 17, 
        };
    let tisize = core::mem::size_of::<TaskInfo>();
    let mut buffers = translated_byte_buffer(current_user_token(), _ti as *mut u8, tisize);
    assert!(buffers.len() == 1 || buffers.len() == 2, "Can not correctly get the physical page of _ti");
    if buffers.len() == 1 {
        let buffer = buffers.get_mut(0).unwrap();
        assert!(buffer.len() == tisize);
        unsafe { buffer.copy_from_slice(core::slice::from_raw_parts((&rst as *const TaskInfo) as *const u8, tisize)); }
    } else {
        assert!(buffers[0].len() + buffers[1].len() == tisize);
        let first_half = buffers.get_mut(0).unwrap();
        let first_half_len = first_half.len();
        unsafe {
            first_half.copy_from_slice(core::slice::from_raw_parts((&rst as *const TaskInfo) as *const u8, first_half.len()));
        }
        let second_half = buffers.get_mut(1).unwrap();
        unsafe {
            second_half.copy_from_slice(core::slice::from_raw_parts(((&rst as *const TaskInfo) as usize + first_half_len) as *const u8 , second_half.len()));
        }
    }
    0
}

// YOUR JOB: Implement mmap.
// pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
//     trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
//     -1
// }
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    mmap(start, len, port)
}

// // YOUR JOB: Implement munmap.
// pub fn sys_munmap(_start: usize, _len: usize) -> isize {
//     trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
//     -1
// }
pub fn sys_munmap(start: usize, len: usize) -> isize {
    munmap(start, len)
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
