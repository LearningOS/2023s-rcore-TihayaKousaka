//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::loader::{get_app_data, get_num_app};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};
use crate::config;
pub use crate::mm;
use crate::timer;



pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    fn update_syscall_times(&self, id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_times[id] += 1;
    }

    /// 得到系统调用次数
    fn get_syscall_times(&self) -> [u32; 500] {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_times
    }

    /// 得到当前任务的开始时间
    fn get_start_time(&self) -> usize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        return timer::get_time_us() - inner.tasks[current].start_time;
    }

    fn mmap(&self, start: usize, len: usize, port: usize) -> isize {
        if (start % config::PAGE_SIZE != 0) || (port & !0x7 != 0) || (port & 0x7 == 0) {
            return -1;
        }

        let start_address = mm::VirtAddr(start);
        let end_address = mm::VirtAddr(start + len);

        let map_permission =
            mm::MapPermission::from_bits((port as u8) << 1).unwrap() | mm::MapPermission::U;

        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        for vpn in mm::VPNRange::new(mm::VirtPageNum::from(start_address), end_address.ceil()) {
            if let Some(pte) = inner.tasks[current].memory_set.translate(vpn) {
                if pte.is_valid() {
                    println!("[debug] This area is used!");
                    return -1;
                }
            };

            println!("[debug] {}", usize::from(vpn));
        }

        inner.tasks[current].memory_set.insert_framed_area(
            start_address,
            end_address,
            map_permission,
        );

        for vpn in mm::VPNRange::new(mm::VirtPageNum::from(start_address), end_address.ceil()) {
            if let None = inner.tasks[current].memory_set.translate(vpn) {
                return -1;
            };
        }

        return 0;
    }

    /// munmap
    fn munmap(&self, start: usize, len: usize) -> isize {
        if start % config::PAGE_SIZE != 0 {
            return -1;
        }

        let start_address = mm::VirtAddr(start);
        let end_address = mm::VirtAddr(start + len);

        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;

        for vpn in mm::VPNRange::new(mm::VirtPageNum::from(start_address), end_address.ceil()) {
            if let None = inner.tasks[current].memory_set.translate(vpn) {
                return -1;
            };

            if let Some(pte) = inner.tasks[current].memory_set.translate(vpn) {
                if pte.is_valid() == false {
                    return -1;
                }
            };
        }

        for vpn in mm::VPNRange::new(mm::VirtPageNum::from(start_address), end_address.ceil()) {
            inner.tasks[current].memory_set.munmap(vpn);
        }

        for vpn in mm::VPNRange::new(mm::VirtPageNum::from(start_address), end_address.ceil()) {
            if let Some(pte) = inner.tasks[current].memory_set.translate(vpn) {
                if pte.is_valid() {
                    println!("[debug] This area is used!");
                    return -1;
                }
            };
        }

        return 0;
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

pub fn get_syscall_times() -> [u32; 500] {
    TASK_MANAGER.get_syscall_times()
}

pub fn update_syscall_times(id: usize) {
    TASK_MANAGER.update_syscall_times(id);
}

pub fn get_current_task_time() -> usize {
    TASK_MANAGER.get_start_time() / 1000
}

/// mmap
pub fn mmap(start: usize, len: usize, port: usize) -> isize {
    TASK_MANAGER.mmap(start, len, port)
}

/// munmap
pub fn munmap(start: usize, len: usize) -> isize {
    TASK_MANAGER.munmap(start, len)
}