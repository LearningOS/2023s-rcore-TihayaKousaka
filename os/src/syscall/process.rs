//! Process management syscalls

use crate::loader::get_app_data_by_name;
use crate::mm::{translated_refmut, translated_str};
use crate::task::set_prio;
#[allow(unused_imports)]
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next, TaskStatus, mmap, unmmap,
};
use crate::timer::get_time_us;
use alloc::sync::Arc;
use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    debug!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

/// Syscall Fork which returns 0 for child process and child_pid for parent process
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

// waiting a subprocess to become a The zombie thread, recyled all source and collect it's return
/// Syscall Exec which accepts the elf path
pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // find a child process

    // ---- access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB lock exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after removing from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB lock automatically
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    let ts_phy_ptr = translated_refmut(current_user_token(), ts);
    unsafe {
        *ts_phy_ptr = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    -1
}

// YOUR JOB: 实现sys_set_priority，为任务添加优先级
pub fn sys_set_priority(_prio: isize) -> isize {
    if _prio <= 1 {
        return -1;
    }

    set_prio(_prio as u8);
    _prio
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, mut _len: usize, _port: usize) -> isize {

    if (_start % PAGE_SIZE) != 0 { return -1; }
    if _port & !0x7 != 0 || _port & 0x7 == 0 { return -1; }

    if _len % PAGE_SIZE != 0 {
        _len = ( _len / PAGE_SIZE + 1 ) * PAGE_SIZE;
    } 
    
    mmap(_start, _len, _port)
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    println!("Inside sys_munmap!!!");
    if _start % PAGE_SIZE != 0 || _len % PAGE_SIZE != 0 {
        println!("out because page not match");
        return -1;
    }

    unmmap(_start, _len) 
}

//
// YOUR JOB: 实现 sys_spawn 系统调用
// ALERT: 注意在实现 SPAWN 时不需要复制父进程地址空间，SPAWN != FORK + EXEC 
pub fn sys_spawn(_path: *const u8) -> isize {
    /* 
    1.  我们设想需要跳过中间的复制父进程空间的操作，因此，我有一个相对简单的设想，我们所创建的新进程
    直接由我们手动设计成为对应的数值，后直接从传入的进程名称中读取对应的elf，并由对应的数据中获取信息。
    2.  在fork中，我们的操作主要通过父进程中的相关数据，以临摹的方式创建出子进程
        3. 其中相对重要的部分在于静态元素（pid, kernel stack的初始化）与inner部分的数据初始化   [ 直接从父进程中获取 ]
        4. 设想是直接通过共有的数据进行初始化                                       
        5. 比如说从elf获取的 `memory_set, user_sp, entry_pointer` 等数据                    [ 由`elf_data`中获取 ]
    */

    
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let current_task = current_task().unwrap();
        let new_task = current_task.create_new_task_from_elf(data);
        let new_pid = new_task.pid.0;

        add_task(new_task);
        new_pid as isize
    } else { -1 }
}
