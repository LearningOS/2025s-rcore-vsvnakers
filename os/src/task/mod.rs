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

pub use context::TaskContext;

use crate::mm::{VirtAddr,MapPermission};
use crate::config::{PAGE_SIZE, PAGE_SIZE_BITS};

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

    /// New Add: ==> sys_id_count
    fn get_syscall_idcount(&self, id: usize) -> usize {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_info[id]
    }

    /// Count the syscall id
    fn count_syscall(&self, id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_info[id] += 1;
    }

    /// function -> mmap
    fn mmap(&self, start: usize, len: usize, port: usize) -> isize {

        if ! VirtAddr::from(start).aligned() || port & !0x7 != 0 || port & 0x7 == 0 {
            return -1; // no aligned
        } else if len == 0 {
            return 0;
        }

        let page_cnt = (len + PAGE_SIZE -1) / PAGE_SIZE;
        
        let vpns: Vec<_> = (0..page_cnt)
            .map(|i| VirtAddr::from(start + i * PAGE_SIZE).floor())
           .collect();
        
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let tcb = &mut inner.tasks[current];
        if tcb.memory_set.check_one_map(&vpns) {
            return -1; //some pages already_mmap
        }


        let  map_perm = MapPermission::U
            | if port & 0b001 != 0 { MapPermission::R } else { MapPermission::empty() }
            | if port & 0b010 != 0 { MapPermission::W } else { MapPermission::empty() }
            | if port & 0b100 != 0 { MapPermission::X } else { MapPermission::empty() };

        tcb.memory_set.my_insert_framed_area(
            VirtAddr::from(start), 
            VirtAddr::from(start + page_cnt * PAGE_SIZE), 
            map_perm) // alloc fail will return -1 
    }

    /// function -> munmap
    fn munmap(&self, start: usize, len: usize) -> isize {
        if ! VirtAddr::from(start).aligned() {
            return -1; // start is no align
        }
        let start = start & !((1 << PAGE_SIZE_BITS) - 1);

        let page_cnt = (len + PAGE_SIZE -1) / PAGE_SIZE;
        if page_cnt == 0 {
            return 0;
        }
       
        // 生成 VPN 列表
        let vpns: Vec<_> = (0..page_cnt)
            .map(|i| VirtAddr::from(start + i * PAGE_SIZE).floor())
            .collect();
        
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let tcb = &mut inner.tasks[current];

        if ! tcb.memory_set.check_all_map(&vpns) {
            // 检查是否都被映射
            return -1; // no already all mmap
        }

        tcb.memory_set.my_remove_framed_page(&vpns);

        0      
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

/// New Add: ==> sys_id_count
pub fn get_syscall_idcount(id: usize) -> usize {
    TASK_MANAGER.get_syscall_idcount(id)
}

/// Count the syscall id
pub fn count_syscall(id: usize) {
    TASK_MANAGER.count_syscall(id);
}

/// function -> mmap
pub fn mmap(start: usize, len: usize, port: usize) -> isize {
    TASK_MANAGER.mmap(start, len, port)
}

/// function -> munmap
pub fn munmap(start: usize, len: usize) -> isize {
    TASK_MANAGER.munmap(start, len)
}
