use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;
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
        // my code
        process_inner.mutex_available[id] = 1;
        for i in 0..process_inner.thread_count() {
            process_inner.mutex_allocation[i][id] = 0;
            process_inner.mutex_need[i][id] = 0;
        }
        // my code

        process_inner.mutex_list[id] = mutex;

        id as isize
    } else {
        // my code
        process_inner.mutex_available.push(1);
        for i in 0..process_inner.thread_count() {
            process_inner.mutex_allocation[i].push(0);
            process_inner.mutex_need[i].push(0);
        }
        // my code

        process_inner.mutex_list.push(mutex);
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
    let tid = current_task().unwrap().get_tid();
    process_inner.mutex_need[tid][mutex_id] += 1; // update need

    let mut lock = true;
    if process_inner.enable_deadlock_detect {
        lock = ! detect_deadlock(&process_inner.mutex_available, 
                                 &process_inner.mutex_allocation, 
                                 &process_inner.mutex_need);
    }

    if lock {
        let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
        drop(process_inner);
        drop(process);
        mutex.lock_with_id(mutex_id);

        0
    } else {
        process_inner.mutex_need[tid][mutex_id] -= 1; // back need

        -0xDEAD
    }
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
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);

    mutex.unlock_with_id(mutex_id);
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
        // my code
        process_inner.semaphore_available[id] = res_count; // notice init val!!
        for i in 0..process_inner.thread_count() {
            process_inner.semaphore_allocation[i][id] = 0;
            process_inner.semaphore_need[i][id] = 0;
        }
        // my code

        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        // my code
        process_inner.semaphore_available.push(res_count); // notice init val!!
        for i in 0..process_inner.thread_count() {
            process_inner.semaphore_allocation[i].push(0);
            process_inner.semaphore_need[i].push(0);
        }
        // my code

        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
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
    let process_inner = process.inner_exclusive_access();

    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    
    drop(process_inner);
    drop(process);

    sem.up(sem_id);

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
    let tid = current_task().unwrap().get_tid();

    process_inner.semaphore_need[tid][sem_id] += 1;

    let mut down = true;
    if process_inner.enable_deadlock_detect {
        down = ! detect_deadlock(&process_inner.semaphore_available, 
                                 &process_inner.semaphore_allocation, 
                                 &process_inner.semaphore_need);
    }

    if down {
        let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
        drop(process_inner);
        sem.down(sem_id);

        0
    } else {
        process_inner.semaphore_need[tid][sem_id] -= 1;

        -0xDEAD
    }
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
    // trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    // -1

    if enabled != 0 && enabled != 1 {
        return -1;
    }

    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.enable_deadlock_detect = if enabled == 0 { false } else { true };
    
    0
}

use alloc::vec;
fn detect_deadlock(available: &Vec<usize>, allocation: &Vec<Vec<usize>>, need: &Vec<Vec<usize>>) -> bool {
    // if will deadlock return true
    let resource_len = available.len();
    let thread_len = allocation.len();

    let mut work = available.clone();
    let mut finish = vec![false; thread_len];

    let mut i = 0;
    while i < thread_len {
        if finish[i] {
            i += 1;
        } else {
            let mut j = 0;
            while j < resource_len {
                if need[i][j] <= work[j] { // resource_j enough
                    j += 1;
                } else { // resource_j not enough
                    break;
                }
            }

            if j == resource_len { // thread_i can finish
                finish[i] = true; // update finish
                for k in 0..resource_len {
                    work[k] += allocation[i][k];
                }
                i = 0; // restart
            } else {
                i += 1;
            }
        }
    }

    for b in finish.iter() {
        if *b == false {
            return true; // deadlock
        }
    }
    false
}