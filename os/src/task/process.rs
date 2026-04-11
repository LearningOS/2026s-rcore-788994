//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use alloc::vec; // 解决 vec! 宏找不到的问题
use core::cell::RefMut;
use crate::mm::translated_refmut;
use alloc::string::String;

/// Process Control Block
pub struct ProcessControlBlock {
    /// Process ID
    pub pid: PidHandle,
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner data of ProcessControlBlock
pub struct ProcessControlBlockInner {
    /// Is the process a zombie?
    pub is_zombie: bool,
    /// Address space
    pub memory_set: MemorySet,
    /// Parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// Child processes
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// Exit code
    pub exit_code: i32,
    /// File descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// Signal flags
    pub signals: SignalFlags,
    /// Tasks (threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// Task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// Mutexes
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// Semaphores
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// Condition variables
    pub condvar_list: Vec<Option<Arc<Condvar>>>,

    /// Is deadlock detection enabled?
    pub is_deadlock_detect: bool,
    /// Record mutex owners
    pub mutex_owner: Vec<Option<usize>>, 
    /// Record which mutex a thread is waiting for
    pub thread_wait_mutex: Vec<Option<usize>>, 
    /// Record resources allocated to each thread
    pub sem_allocated: Vec<alloc::collections::BTreeMap<usize, usize>>, 
    /// Record which semaphore a thread is waiting for
    pub thread_wait_sem: Vec<Option<usize>>, 
}

impl ProcessControlBlockInner {
    /// Get user token
    #[allow(unused)]
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// Allocate a file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// Allocate a thread id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// Deallocate a thread id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// Get thread count
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// Get a task by tid
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }

    /// Check for deadlock
    pub fn check_deadlock(&self, current_tid: usize, wait_mutex: Option<usize>, wait_sem: Option<usize>) -> bool {
        let mut visited_threads = alloc::vec::Vec::new();
        let mut queue = alloc::vec::Vec::new();
        
        // 1. 初始依赖推入
        if let Some(m_id) = wait_mutex {
            if let Some(owner) = self.mutex_owner.get(m_id).unwrap_or(&None) {
                // 如果是自己持有了锁又去申请（自死锁），直接返回 true
                if *owner == current_tid { return true; }
                queue.push(*owner);
            }
        }
        if let Some(s_id) = wait_sem {
            if let Some(map) = self.sem_allocated.get(s_id) {
                for (&owner, &count) in map.iter() {
                    if count > 0 {
                        // 即使是自己持有信号量，如果此时 count=0 且没别人能 up，也会死锁
                        queue.push(owner);
                    }
                }
            }
        }
        
        // 2. BFS 搜索依赖环
        while let Some(t) = queue.pop() {
            if t == current_tid { return true; } // 发现环路
            if visited_threads.contains(&t) { continue; }
            visited_threads.push(t);
            
            // --- 核心优化点 ---
            // 获取线程 t 的控制块
            if let Some(Some(t_task)) = self.tasks.get(t) {
                let t_inner = t_task.inner_exclusive_access();
                // 如果线程 t 当前不是 Blocked 状态，说明它还在跑，
                // 它可能会执行 mutex_unlock 或 sem_up，从而打破潜在的死锁。
                if t_inner.task_status != super::TaskStatus::Blocked {
                    continue; 
                }
            }
            // -----------------

            // 继续追踪线程 t 正在等待的资源
            if let Some(Some(m_id)) = self.thread_wait_mutex.get(t) {
                if let Some(owner) = self.mutex_owner.get(*m_id).unwrap_or(&None) {
                    queue.push(*owner);
                }
            }
            if let Some(Some(s_id)) = self.thread_wait_sem.get(t) {
                if let Some(map) = self.sem_allocated.get(*s_id) {
                    for (&owner, &count) in map.iter() {
                        if count > 0 {
                            queue.push(owner); 
                        }
                    }
                }
            }
        }
        false
    }
}
impl ProcessControlBlock {
    /// Get exclusive access to inner data
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// Create a new process
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        Some(Arc::new(Stdin)),
                        Some(Arc::new(Stdout)),
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    is_deadlock_detect: false,
                    mutex_owner: Vec::new(),
                    thread_wait_mutex: Vec::new(),
                    sem_allocated: Vec::new(),
                    thread_wait_sem: Vec::new(),
                })
            },
        });
        let task = Arc::new(TaskControlBlock::new(Arc::clone(&process), ustack_base, true));
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point, ustack_top, KERNEL_SPACE.exclusive_access().token(),
            kstack_top, trap_handler as usize,
        );
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        add_task(task);
        process
    }

    /// Exec a new program
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        self.inner_exclusive_access().memory_set = memory_set;
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(new_token, (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize)
            }).collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        user_sp -= user_sp % core::mem::size_of::<usize>();
        let mut trap_cx = TrapContext::app_init_context(
            entry_point, user_sp, KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(), trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Fork a new process
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        let pid = pid_alloc();
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    is_deadlock_detect: false,
                    mutex_owner: Vec::new(),
                    thread_wait_mutex: Vec::new(),
                    sem_allocated: Vec::new(),
                    thread_wait_sem: Vec::new(),
                })
            },
        });
        parent.children.push(Arc::clone(&child));
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent.get_task(0).inner_exclusive_access().res.as_ref().unwrap().ustack_base(),
            false,
        ));
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        add_task(task);
        child
    }
    
    /// Get process id
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}