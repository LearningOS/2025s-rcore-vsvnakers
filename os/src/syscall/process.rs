//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
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
    let us = crate::timer::get_time_us();
    let timeval = TimeVal {
        sec: us / 1_000_000,  
        usec: us % 1_000_000, 
    };
    // ch3::sys_get_time
    
    // 将timeval结构体的地址转换为usize类型，用于后续数据复制
    let mut ptr = &timeval as *const TimeVal as usize;

    // 将用户空间的ts指针翻译为内核可以访问的缓冲区，处理可能跨页的情况
    // current_user_token获取当前任务的页表token，size_of获取TimeVal结构体大小
    let mut buffers = crate::mm::translated_byte_buffer(
        crate::task::current_user_token(), 
        ts as *const u8, 
        core::mem::size_of::<TimeVal>());
    // 遍历所有缓冲区(可能有多个，如果TimeVal跨页)
    for buffer in buffers.iter_mut() {
        // 从内核时间数据创建一个字节切片，长度与当前buffer相同
        let data = unsafe {core::slice::from_raw_parts(ptr as *const u8, buffer.len()) };
        // 更新指针位置，为下一个buffer准备(如果TimeVal跨页)
        ptr += buffer.len();

        // 将内核时间数据复制到用户空间缓冲区
        buffer.copy_from_slice(data);
    }
    // 返回0表示系统调用成功
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// 0: Read memory 1: Write memory 2: Get id times
use crate::mm::{PTEFlags, PageTable, VirtAddr, VirtPageNum};

pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    match trace_request {
        // 读取用户空间内存
        0 => {
            let read_addr = id as *const u8; // 将 id 作为用户空间地址
            let page_table = PageTable::from_token(crate::task::current_user_token()); // 获取当前用户页表
            let vpn: VirtPageNum = VirtAddr::from(read_addr as usize).floor(); // 计算虚拟页号

            // 检查页表项是否存在并具有有效的权限
            if let Some(pte) = page_table.translate(vpn) {
                let flags = pte.flags();
                if pte.is_valid() && (flags & PTEFlags::U) != PTEFlags::empty() && pte.readable() {
                    // 翻译用户空间地址并读取数据
                    let buffers = crate::mm::translated_byte_buffer(
                        crate::task::current_user_token(),
                        read_addr,
                        core::mem::size_of::<u8>(),
                    );
                    return buffers[0][0] as isize; // 返回读取的字节数据
                }
            }
            -1 // 如果检查失败，返回 -1 表示错误
        }
        // 写入用户空间内存
        1 => {
            let write_addr = id as *mut u8; // 将 id 作为用户空间地址
            let page_table = PageTable::from_token(crate::task::current_user_token()); // 获取当前用户页表
            let vpn: VirtPageNum = VirtAddr::from(write_addr as usize).floor(); // 计算虚拟页号

            // 检查页表项是否存在并具有有效的权限
            if let Some(pte) = page_table.translate(vpn) {
                let flags = pte.flags();
                if pte.is_valid() && (flags & PTEFlags::U) != PTEFlags::empty() && pte.writable() {
                    // 翻译用户空间地址并写入数据
                    let mut buffers = crate::mm::translated_byte_buffer(
                        crate::task::current_user_token(),
                        write_addr,
                        core::mem::size_of::<u8>(),
                    );
                    buffers[0][0] = data as u8; // 写入数据
                    return 0; // 返回 0 表示成功
                }
            }
            -1 // 如果检查失败，返回 -1 表示错误
        }
        // 获取系统调用计数
        2 => crate::task::get_syscall_idcount(id) as isize, // 返回指定系统调用的计数
        // 未知请求
        _ => -1, // 返回 -1 表示未知请求
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    crate::task::mmap(start, len, port)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    crate::task::munmap(start, len)
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
