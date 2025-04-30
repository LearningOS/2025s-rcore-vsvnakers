//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

use crate::task::current_process;

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self, sem_id: usize) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;

        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        let tid = current_task().unwrap().get_tid();

        process_inner.semaphore_available[sem_id] += 1;
        process_inner.semaphore_allocation[tid][sem_id] -= 1;
        drop(process_inner);
        drop(process);

        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self, sem_id: usize) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;

        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        }

        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        let tid = current_task().unwrap().get_tid();

        process_inner.semaphore_available[sem_id] -= 1;
        process_inner.semaphore_allocation[tid][sem_id] += 1;
        process_inner.semaphore_need[tid][sem_id] -= 1;
    }
}
