
use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::borrow::ToOwned;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use crate::sync::UPSafeCell;
use lazy_static::*;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        process_inner.available[id] = 1;
        let tid_allocation = &mut process_inner.allocation;
        tid_allocation.iter_mut().for_each(|line| line[id] = 0);
        let tid_need = &mut process_inner.need;
        tid_need.iter_mut().for_each(|line| line[id] = 0);

        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.available.push(1);
        //每把锁加新列
        let tid_allocation = &mut process_inner.allocation;
        tid_allocation.iter_mut().for_each(|line| line.push(0));
        let tid_need = &mut process_inner.need;
        tid_need.iter_mut().for_each(|line| line.push(0));
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    let mut need_reset = 0;
    let mut allocation_reset = 0;
    let mut available_reset = 0;
    if let Some(task) = current_task() {
        if let Some(res) = &task.inner_exclusive_access().res {
            if mutex.is_taked() {
                need_reset = process_inner.need[res.tid][mutex_id];
                allocation_reset = process_inner.allocation[res.tid][mutex_id];
                available_reset = process_inner.available[mutex_id];
                process_inner.need[res.tid][mutex_id] = 1;
            }            
            else {
                need_reset = process_inner.need[res.tid][mutex_id];
                allocation_reset = process_inner.allocation[res.tid][mutex_id];
                available_reset = process_inner.available[mutex_id];
                process_inner.allocation[res.tid][mutex_id] = 1;
                process_inner.available[mutex_id] = 0;
            }
        }
    }

    //死锁检测
    if DLOCK_DETECT_ENABLE.exclusive_access().enable {
        if deadlock_detect(&process_inner.available, &process_inner.allocation, &process_inner.need) {
            if let Some(task) = current_task() {
                //操作取消，重置状态
                if let Some(res) = &task.inner_exclusive_access().res {
                    process_inner.allocation[res.tid][mutex_id] = allocation_reset;
                    process_inner.need[res.tid][mutex_id] = need_reset;
                    process_inner.available[mutex_id] = available_reset;
                }
            }
            return -0xdead;
        }
    }

    drop(process_inner);
    drop(process);
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    if let Some(task) = current_task() {
        if let Some(res) = &task.inner_exclusive_access().res {
            let tid_need = &mut process_inner.need[res.tid];
            tid_need[mutex_id] = 0;
            let tid_allocation = &mut process_inner.allocation[res.tid];
            tid_allocation[mutex_id] = 0;
            process_inner.available[mutex_id] = 1;
        }
    }
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.available[id] = res_count;
        let tid_allocation = &mut process_inner.allocation;
        tid_allocation.iter_mut().for_each(|line| line[id] = 0);
        let tid_need = &mut process_inner.need;
        tid_need.iter_mut().for_each(|line| line[id] = 0);
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.available.push(res_count);
        //每个信号量加新列
        let tid_allocation = &mut process_inner.allocation;
        tid_allocation.iter_mut().for_each(|line| line.push(0));
        let tid_need = &mut process_inner.need;
        tid_need.iter_mut().for_each(|line| line.push(0));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    if let Some(task) = current_task() {
        if let Some(res) = &task.inner_exclusive_access().res {
            process_inner.allocation[res.tid][sem_id] -= 1;
            process_inner.available[sem_id] += 1;
        }
    }
    drop(process_inner);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    
    let mut need_reset = 0;
    let mut allocation_reset = 0;
    let mut available_reset = 0;

    if let Some(task) = current_task() {
        if let Some(res) = &task.inner_exclusive_access().res {
            if sem.inner.exclusive_access().count > 0{
                need_reset = process_inner.need[res.tid][sem_id];
                allocation_reset = process_inner.allocation[res.tid][sem_id];
                available_reset = process_inner.available[sem_id];
                process_inner.available[sem_id] -= 1;
                process_inner.allocation[res.tid][sem_id] += 1;
                if process_inner.need[res.tid][sem_id] > 0 {
                    process_inner.need[res.tid][sem_id] -= 1;
                }
            }
            else {
                need_reset = process_inner.need[res.tid][sem_id];
                allocation_reset = process_inner.allocation[res.tid][sem_id];
                available_reset = process_inner.available[sem_id];
                process_inner.need[res.tid][sem_id] += 1;
            }
        }
    }

    //死锁检测
    if DLOCK_DETECT_ENABLE.exclusive_access().enable {
        if deadlock_detect(&process_inner.available, &process_inner.allocation, &process_inner.need) {
            if let Some(task) = current_task() {
                //操作取消，重置状态
                if let Some(res) = &task.inner_exclusive_access().res {
                    process_inner.allocation[res.tid][sem_id] = allocation_reset;
                    process_inner.need[res.tid][sem_id] = need_reset;
                    process_inner.available[sem_id] = available_reset;
                }
            }
            return -0xdead;
        }
    }
    drop(process_inner);
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    match enabled {
        1 => DLOCK_DETECT_ENABLE.exclusive_access().enable = true,
        0 => DLOCK_DETECT_ENABLE.exclusive_access().enable = false,
        _ => (),
    }
    0
}

lazy_static! {
    pub static ref DLOCK_DETECT_ENABLE: UPSafeCell<DeadlockDetectEnable> = unsafe {UPSafeCell::new(DeadlockDetectEnable::new())};
}

pub struct DeadlockDetectEnable {
    pub enable: bool,
}

impl DeadlockDetectEnable {
    fn new() ->Self {
        Self {
            enable: false,
        }
    }
}

pub fn deadlock_detect(available: &Vec::<usize>,allocation: &[Vec<usize>],need: &[Vec<usize>]) -> bool {
    let mut work = available.to_owned();
    let len = work.len();
    let mut finish = vec![false; len];

    for _ in 0..len {
        let mut flag = false;
        for i in 0..len {
            // println!("finish:");
            // finish.iter().for_each(|c| print!("{} ",c));println!("");            
            // println!("need[{}]:",i);
            // need[i].iter().for_each(|c| print!("{} ",c));println!("");   
            // println!("work:");
            // work.iter().for_each(|c| print!("{} ",c));println!("");   
            // println!("allocation[{}]:",i);
            // allocation[i].iter().for_each(|c| print!("{} ",c));println!("");   
            if finish[i] == false 
            && (need[i].iter().zip(work.iter()).find(|(&n, &w)| n > w)).is_none() {
                work.iter_mut().zip(allocation[i].iter()).for_each(|(w, a)| *w += *a);
                finish[i] = true;
                flag = true;
            }
        }
        if !flag {
            for i in 0..len {
                if finish[i] == false{
                    //发生死锁
                    return true;
                }
            }
            // 没发生死锁
            return false;
        }
    }
    false
} 