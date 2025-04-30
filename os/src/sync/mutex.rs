//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

use crate::task::current_process; // my code

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);

    ///
    fn lock_with_id(&self, _mutex_id: usize);

    ///
    fn unlock_with_id(&self, _mutex_id: usize);
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                return;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
    }

    ///
    fn lock_with_id(&self, _mutex_id: usize) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                return;
            }
        }
    }

    ///
    fn unlock_with_id(&self, _mutex_id: usize) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
    }

}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();

        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true; // alloc res
        }
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }

    fn lock_with_id(&self, _mutex_id: usize) {
        let mut mutex_inner = self.inner.exclusive_access();

        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();

            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            let tid = current_task().unwrap().get_tid();

            process_inner.mutex_allocation[tid][_mutex_id] += 1;
            process_inner.mutex_need[tid][_mutex_id] -= 1;
        } else {
            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            let tid = current_task().unwrap().get_tid();
            
            process_inner.mutex_allocation[tid][_mutex_id] += 1;
            process_inner.mutex_need[tid][_mutex_id] -= 1;

            process_inner.mutex_available[_mutex_id] -= 1;
            mutex_inner.locked = true; // alloc res
        }
    }

    fn unlock_with_id(&self, _mutex_id: usize) {
        trace!("kernel: MyMutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);

        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        let tid = current_task().unwrap().get_tid();

        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            process_inner.mutex_allocation[tid][_mutex_id] -= 1;
            drop(process_inner);
            drop(process);

            wakeup_task(waking_task);
        } else {
            process_inner.mutex_allocation[tid][_mutex_id] -= 1;

            process_inner.mutex_available[_mutex_id] += 1;
            mutex_inner.locked = false;
        }
    }
}
