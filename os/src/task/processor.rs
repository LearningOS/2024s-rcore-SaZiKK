//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::manager::fetch_task;
use super::TaskStatus;
use super::{TaskContext, TaskControlBlock};
use crate::config::{BIG_STRIDE, MAX_SYSCALL_NUM};
use crate::mm::{MapPermission, VirtAddr};
use crate::sync::UPSafeCell;
use crate::timer::get_time_ms;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// Processor management structure
pub struct Processor {
    ///The task currently executing on the current processor
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    ///Create an empty Processor
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }

    /// get current task process time
    pub fn get_task_time(&self) -> usize {
        let current = self.current();
        if let Some(task) = current {
            let total_time = get_time_ms() - task.inner_exclusive_access().start_time;
            total_time
        } else {
            println!("no tasks running now, why you can get time, WTF");
            0
        }
    }

    /// get syscall times
    pub fn get_syscall_times(&self) -> [u32; MAX_SYSCALL_NUM] {
        let current = self.current();
        if let Some(task) = current {
            task.inner_exclusive_access().syscall_times
        } else {
            println!("no tasks running now, why you can get syscall times, WTF");
            [0; MAX_SYSCALL_NUM]
        }
    }

    /// count syscall time
    pub fn count_syscall_times(&self, syscall_id: usize){
        let current = self.current();
        if let Some(task) = current {
            task.inner_exclusive_access().syscall_times[syscall_id] += 1;
        } else {
            println!("no tasks running now, why you can count syscall times, WTF");
        }
    }

    /// alloc space
    pub fn alloc_new_space(&self, start_va: VirtAddr,end_va: VirtAddr, permission: MapPermission) -> bool {
        let current = self.current();
        if let Some(task) = current {
            if !task.inner_exclusive_access().memory_set.space_check(start_va, end_va) {
                return false;
            }
            task.inner_exclusive_access().memory_set.insert_framed_area(start_va, end_va, permission);
            true
        } else {
            println!("no tasks running now, why you alloc space, WTF");
            false
        }
    }
    
    /// dealloc space
    pub fn dealloc_space(&self, start_va: VirtAddr,end_va: VirtAddr) -> bool {
        let current = self.current();
        if let Some(task) = current {
            task.inner_exclusive_access().memory_set.unmap_space(start_va, end_va)
        } else {
            println!("no tasks running now, why you dealloc space, WTF");
            false
        }
    }

    ///set priority
    pub fn set_priority(&self, priority: usize) {
        let current = self.current();
        if let Some(task) = current {
            task.inner_exclusive_access().priority = priority;
        } else {
            println!("no tasks running now, why you can set priority, WTF");
        }
    }

}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;

            if task_inner.start_time == 0 {
                task_inner.start_time = get_time_ms();
            }
            //更新stride
            task_inner.stride += BIG_STRIDE / task_inner.priority;

            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

///Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

///Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}
