//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{TaskContext, TaskControlBlock};
//use crate::config::PAGE_SIZE;
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;
//use riscv::addr::page;

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

    /// Processor -> Memory Mapping
    fn mmap(&self, start: usize, len: usize, port: usize) -> isize {
        // Check args 参数校验
        use crate::mm::VirtAddr;
        use crate::config::PAGE_SIZE;

        if !VirtAddr::from(start).aligned() || port & !0x7 != 0
            || port & 0x7 == 0 {
                return -1; // no valid port
            } else if len == 0 {
                return 0;
            }

        // Get page count 计算映射页数
        let page_cnt = (len + PAGE_SIZE - 1) / PAGE_SIZE;

        // Get map vpns 构造 VPN 列表（要映射的虚拟页号）
        use::alloc::vec::Vec;
        let vpns: Vec<_> = (0..page_cnt)
            .map(|i| VirtAddr::from(start + i * PAGE_SIZE).floor())
            .collect();

        // Check if already mapped 检查是否已经映射
        let tcb = self.current().unwrap();
        let mut tcb = tcb.inner_exclusive_access();
        if tcb.memory_set.already_mmaped(&vpns) {
            return -1; // already mapped
        }

        // Permission Handling 权限处理
        use crate::mm::MapPermission;
        let  map_perm = MapPermission::U
            | if port & 0b001 != 0 { MapPermission::R } else { MapPermission::empty() }
            | if port & 0b010 != 0 { MapPermission::W } else { MapPermission::empty() }
            | if port & 0b100 != 0 { MapPermission::X } else { MapPermission::empty() };

        // Map pages 正式插入映射
        tcb.memory_set.my_insert_framed_area(
            VirtAddr::from(start),
            VirtAddr::from(start + page_cnt * PAGE_SIZE),
            map_perm
        )
    }

    /// Processor -> Memory Unmapping
    fn munmap(&self, start: usize, len: usize) -> isize {
        use crate::mm::VirtAddr;
        use crate::config::PAGE_SIZE;
        
        // Check args 参数校验
        if !VirtAddr::from(start).aligned() {
            return -1;
        }

        // Get page count 对齐 start，计算页数
        let start = start & !((1 << crate::config::PAGE_SIZE_BITS) - 1);
        let page_cnt = (len + PAGE_SIZE - 1) / PAGE_SIZE;
        if page_cnt == 0 { return 0; }

        // Build list of virtual page numbers to unmap 构造要解除映射的虚页号列表
        use::alloc::vec::Vec;
        let vpns: Vec<_> = (0..page_cnt)
            .map(|i| VirtAddr::from(start + i * PAGE_SIZE).floor())
            .collect();

        // Get current task control block and access memory set 获取当前进程控制块，进入内部数据结构
        let tcb = self.current().unwrap();
        let mut tcb = tcb.inner_exclusive_access();

        // Check that all pages are currently mapped
        if !tcb.memory_set.already_all_mmaped(&vpns) {
            return -1; // not all mapped
        }

        // Memory Unmap 调用 remove_framed_page 移除映射
        tcb.memory_set.my_remove_framed_page(&vpns);

        0
    }
}


lazy_static! {
    #[allow(missing_docs)]
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

/// Memory Mapping
pub fn mmap(start: usize, len: usize, port: usize) -> isize{
    PROCESSOR.exclusive_access().mmap(start, len, port)
}

/// Memory Unmap
pub fn munmap(start: usize, len: usize) -> isize {
    PROCESSOR.exclusive_access().munmap(start, len)
}