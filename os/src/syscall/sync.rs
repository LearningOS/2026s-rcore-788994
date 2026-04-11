use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

pub fn sys_sleep(ms: usize) -> isize {
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner.mutex_list.iter().enumerate().find(|(_, item)| item.is_none()).map(|(id, _)| id) {
        process_inner.mutex_list[id] = mutex;
        if id >= process_inner.mutex_owner.len() {
            process_inner.mutex_owner.resize(id + 1, None);
        }
        process_inner.mutex_owner[id] = None;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_owner.push(None);
        process_inner.mutex_list.len() as isize - 1
    }
}

///
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let process = current_process();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    
    // 确保数组长度
    if tid >= process_inner.thread_wait_mutex.len() {
        process_inner.thread_wait_mutex.resize(tid + 1, None);
        process_inner.thread_wait_sem.resize(tid + 1, None);
    }

    if process_inner.is_deadlock_detect {
        // 检查自死锁：如果当前线程已经是该锁的所有者
        if process_inner.mutex_owner[mutex_id] == Some(tid) {
            return -0xDEAD;
        }
        // 检查环路死锁
        if process_inner.check_deadlock(tid, Some(mutex_id), None) {
            return -0xDEAD;
        }
    }

    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    
    // 如果锁已被占用，标记当前线程进入等待状态
    if process_inner.mutex_owner[mutex_id].is_some() {
        process_inner.thread_wait_mutex[tid] = Some(mutex_id);
    }
    
    drop(process_inner);
    mutex.lock(); // 此处可能阻塞
    
    // 阻塞回来后，说明拿到了锁
    let mut process_inner = process.inner_exclusive_access();
    process_inner.thread_wait_mutex[tid] = None; // 清除等待标记
    process_inner.mutex_owner[mutex_id] = Some(tid); // 设置所有者
    0
}
///
pub fn sys_semaphore_create(res_count: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner.semaphore_list.iter().enumerate().find(|(_, item)| item.is_none()).map(|(id, _)| id) {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        if id >= process_inner.sem_allocated.len() {
            process_inner.sem_allocated.resize(id + 1, alloc::collections::BTreeMap::new());
        }
        process_inner.sem_allocated[id].clear();
        id
    } else {
        process_inner.semaphore_list.push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.sem_allocated.push(alloc::collections::BTreeMap::new());
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
///
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let process = current_process();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    
    if tid >= process_inner.thread_wait_sem.len() {
        process_inner.thread_wait_mutex.resize(tid + 1, None);
        process_inner.thread_wait_sem.resize(tid + 1, None);
    }

    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    
    let will_block = sem.inner.exclusive_access().count <= 0;
    if process_inner.is_deadlock_detect && will_block {
        if process_inner.check_deadlock(tid, None, Some(sem_id)) {
            return -0xDEAD; 
        }
    }

    if will_block {
        process_inner.thread_wait_sem[tid] = Some(sem_id);
    }
    
    drop(process_inner);
    sem.down(); // 可能阻塞
    
    let mut process_inner = process.inner_exclusive_access();
    process_inner.thread_wait_sem[tid] = None;
    let count = process_inner.sem_allocated[sem_id].entry(tid).or_insert(0);
    *count += 1;
    
    // 这里非常关键：必须返回 0 表示成功获取了信号量！
    0
}


///
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let process = current_process();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    let mut process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    
    if let Some(count) = process_inner.sem_allocated[sem_id].get_mut(&tid) {
        if *count > 0 { *count -= 1; }
    }
    drop(process_inner);
    drop(process);
    sem.up();
    0
}
///
pub fn sys_condvar_create() -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner.condvar_list.iter().enumerate().find(|(_, item)| item.is_none()).map(|(id, _)| id) {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner.condvar_list.push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
///
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
///
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
///
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.is_deadlock_detect = enabled != 0;
    0
}


///
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    // 确保锁存在且当前线程是持有者（简单处理直接取锁）
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    
    // 清除持有者状态
    process_inner.mutex_owner[mutex_id] = None;
    
    drop(process_inner);
    drop(process);
    
    mutex.unlock();
    0
}