//! File and filesystem-related syscalls
use crate::fs::{open_file, OpenFlags, Stat, link_file, unlink_file};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer, };
use crate::task::{current_task, current_user_token};
//first version
//use crate::mm::translated_refmut;
//use crate::fs::count_nlink;
//end here
use crate::fs::get_hard_links_by_inode_number;
use crate::fs::StatMode;
use crate::mm::VirtAddr;
use crate::task::translate;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
// pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
//     //println!("begin sys fstat");
//     let token=current_user_token();
//     let st=translated_refmut(token, _st);
//     let task=current_task().unwrap();
//     let inner=task.inner_exclusive_access();
//     //println!("begin get info");
//     if _fd>2&&_fd<inner.fd_table.len(){
//         st.dev=0;
//         let ino=inner.get_ino(_fd);
//         //println!("end get ino");
//         st.ino=ino as u64;
//         st.mode=inner.get_file_type(_fd);
//         //println!("end get file mode");
//         st.nlink=count_nlink(ino);
//         //println!("end count nlink");
//         0
//     }else{
//         -1
//     }
// }
// 获取指定文件描述符所关联的文件的状态信息并存储在传入的指向 Stat 结构体的指针 _st 所指向的内存中
// 将与指定文件描述符所关联的文件的状态信息存储在指向 Stat 结构体的指针 _st 所指向的内存中。
// 在函数中，首先获取当前进程的任务结构体，并获取该任务的文件描述符表的排他访问权。
// 然后检查文件描述符的合法性，如果文件描述符非法，则返回 -1；否则获取与之关联的 inode，获取该 inode 的编号
// 比较难！
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if _fd >= inner.fd_table.len(){
        return -1;
    }
    if let Some(inode) = &inner.fd_table[_fd]{
        let ino = inode.get_inode_number();
        let nlink = get_hard_links_by_inode_number(ino as u32) as u32;
        let t = inode.get_type();
        let mode = if t == 0{StatMode::DIR}else{StatMode::FILE};

        drop(inner);
        let vaddr = _st as usize;
        let vaddr_obj = VirtAddr(vaddr);
        let page_off = vaddr_obj.page_offset();
    
        let vpn = vaddr_obj.floor();
    
        let ppn = translate(vpn);
    
        let paddr : usize = ppn.0 << 12 | page_off;
        let st = paddr as *mut Stat;

        unsafe {
            (*st).ino = ino as u64;
            (*st).nlink = nlink;
            (*st).mode = mode;
        }
        return 0;

    }else{
        return -1;
    }
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    let token=current_user_token();
    let old_name=translated_str(token, _old_name);
    let new_name=translated_str(token, _new_name);
    link_file(old_name.as_str(),new_name.as_str())
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    let token=current_user_token();
    let name=translated_str(token, _name);
    unlink_file(name.as_str())
}
