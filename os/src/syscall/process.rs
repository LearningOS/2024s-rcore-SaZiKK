//! Process management syscalls

use core::{borrow::BorrowMut, mem::size_of, ptr};


use crate::{mm::translated_byte_buffer, task::{current_user_token, dealloc_current_space}};
#[allow(unused)]
use crate::{
    config::MAX_SYSCALL_NUM, mm::{MapPermission, VirtAddr}, task::{
        change_program_brk, count_syscall_times_current, exit_current_and_run_next, get_current_task_time, insert_new_framed_area, suspend_current_and_run_next, TaskStatus
    }, timer::get_time_us
};

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

/// task exitime_val_slice and submit an exit code
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
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let token = current_user_token();
    let slices = translated_byte_buffer(token, ts as *const u8, size_of::<TimeVal>());
    let mut ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    unsafe {
        let time_val_slice = ts.borrow_mut() as *mut TimeVal;
        let time_val_bytes: &[u8] = core::slice::from_raw_parts(
            time_val_slice as *const u8,
            size_of::<TimeVal>()
        );
        let mut offset = 0;
        for slice in slices {
            let slice_len = slice.len();
            slice.copy_from_slice(&time_val_bytes[offset..offset + slice_len]);
            offset += slice_len;
        }
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    //id设置为超过上限的数表示查询
    let syscall_times = count_syscall_times_current(MAX_SYSCALL_NUM + 1);
    let time = get_current_task_time();
    let mut v = translated_byte_buffer(current_user_token(), _ti as *const u8, size_of::<TaskInfo>());
    let mut ti = TaskInfo{
    status: TaskStatus::Running,
    syscall_times,
    time,
    }; 
    unsafe{
        let mut p = ti.borrow_mut() as *mut TaskInfo as *mut u8;
        for slice in v.iter_mut() {
            let len = slice.len();
            ptr::copy_nonoverlapping(p, slice.as_mut_ptr(), len);
            p = p.add(len);
        }
    } 
    0
}



// YOUR JOB: Implement mmap.
#[allow(unused)]
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    let start_va = VirtAddr::from(_start);
    let end_va = VirtAddr::from(_start + _len);
    let permission =  MapPermission::from_bits((_port << 1 | 16) as u8).unwrap();
    if _port & !0x7 != 0 || _port & 0x7 == 0 {
       return -1;
    }
    if !start_va.aligned(){
        return -1;
    }
    if !insert_new_framed_area(start_va, end_va, permission) {
        return -1;
    }
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    let start_va: VirtAddr = _start.into();
    let end_va: VirtAddr = (_start + _len).into();
    if dealloc_current_space(start_va, end_va) {
        return 0
    }
    -1
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
