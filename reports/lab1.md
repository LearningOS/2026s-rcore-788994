# rCore 实验报告


## 2. 实验内容总结
本实验（Chapter 5 / Lab1）主要完成了以下几个核心机制的实现：
1. **进程创建 (sys_spawn)**：实现了比 `fork + exec` 更高效的进程生成机制。
2. **内存映射 (sys_mmap / sys_munmap)**：实现了用户态动态申请和释放物理内存映射的系统调用。
3. **进程调度 (Stride Scheduling)**：实现了基于步长 (Stride) 和优先级 (Priority) 的公平调度算法。

## 3. 核心功能实现思路

### 3.1 内存映射 (mmap 与 munmap)
- **mmap**：首先校验传入参数的合法性（是否按页对齐，权限位 `port` 是否合法且包含在 rwx 中）。对于重叠检查，遍历需要映射的每一页虚拟页号，调用 `memory_set.translate(vpn)`，如果发现其页表项存在**且 Valid 位为 1**，则判定为重叠并返回 -1。最后通过 `insert_framed_area` 为用户进程动态分配物理页并建立映射。
- **munmap**：除了常规对齐校验外，为了与 `MapArea` 抽象兼容，在内核中新增了 `remove_area_with_start_and_end_vpn` 方法，实现对给定起始和结束页号的精准匹配和删除，一旦解映射成功底层会自动回收物理帧。

### 3.2 进程生成 (sys_spawn)
- `spawn` 的核心是避免像 `fork` 那样复制不必要的数据。
- 在实现中，先克隆出原任务的控制块，接着调用 `MemorySet::from_elf(data)` 解析目标 ELF 文件生成新的地址空间，并替换掉子进程的 `memory_set`。
- **关键点**：替换地址空间后，必须重新获取新地址空间中 `TrapContext` 的物理页号 (`trap_cx_ppn`)，并使用新 ELF 的 `entry_point` 和 `user_sp` 更新上下文，否则会导致严重的 Page Fault。

### 3.3 步长调度算法 (Stride Scheduling)
- 在 `TaskControlBlockInner` 中新增了 `priority` 和 `pass` 属性。
- 修改了 `TaskManager::fetch` 的逻辑：每次遍历 `ready_queue`，挑选出当前 `pass` 值最小的进程。
- 选中进程后，将其 `pass` 值增加 `BigStride / priority`（为防止除 0 异常，设定最低优先级为 2）。这保证了高优先级的进程 `pass` 增长得慢，从而获得更多的 CPU 时间片。

## 4. 遇到的困难与解决方案
1. **mmap 重叠检测的误判问题**：
   - **问题**：最初在使用 `translate` 检查页表时，只要有页表项结构就认为被占用，导致 `munmap` 后紧接 `mmap` 会失败。
   - **解决**：深入理解了 rCore 底层的页表机制，发现 `munmap` 仅清空了 PTE 的标志位而并未销毁结构。在判断时加上了 `pte.is_valid()` 条件，完美解决了复用判定问题。
2. **sys_spawn 时的崩溃**：
   - **问题**：初步实现 `spawn` 后测试程序疯狂报 `StorePageFault` 错误。
   - **解决**：排查后发现是没有更新新地址空间对应的 `trap_cx_ppn`，导致内核将 Trap 上下文写到了旧地址空间的物理页上。补充翻译获取新的 PPN 后，程序成功运行。

## 5. 实验心得
通过本次实验，我深入理解了操作系统内核是如何通过页表管理虚拟内存的，也明白了进程调度的底层逻辑以及 `MapArea`、`MemorySet` 等数据结构的生命周期管理。