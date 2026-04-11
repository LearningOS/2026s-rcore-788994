# Lab 1 实验报告


## 1. 实验目的

本实验是操作系统内核实现的基础阶段，主要目的是：
1. 熟悉 Rust 语言在裸机（Bare-metal）环境下的编程范式。
2. 掌握 RISC-V 架构下的特权级机制（U-Mode 与 S-Mode 的切换）。
3. 理解并实现操作系统的核心机制：**陷入（Trap）处理**与**上下文切换（Context Switch）**。
4. （如果是批处理系统：实现简单的批处理操作系统，使得内核能够按照顺序加载并运行多个用户态程序。）

## 2. 实验环境

* **操作系统**：Ubuntu 20.04 / 22.04 (或 WSL2)
* **编译器**：Rust Nightly (包含 `riscv64gc-unknown-none-elf` target)
* **模拟器**：QEMU emulator version 7.0.0+ (riscv64)
* **构建工具**：Make, Cargo

## 3. 实验核心内容与实现原理

### 3.1 特权级切换与 Trap 机制
在 RISC-V 架构中，当用户程序（U-Mode）执行系统调用（如 `ecall`）或触发异常时，CPU 会将特权级切换至 S-Mode，并跳转到 `stvec` 寄存器所指向的内核处理地址。

我们在 `os/src/trap/trap.S` 中实现了汇编级别的上下文保存与恢复：
* **保存上下文 (`__alltraps`)**：在进入内核态时，将 32 个通用寄存器以及 `sstatus`、`sepc` 压入内核栈中，保存到 `TrapContext` 结构体中。
* **恢复上下文 (`__restore`)**：处理完系统调用或异常后，从内核栈中恢复这些寄存器的值，并使用 `sret` 指令返回用户态。

### 3.2 关键代码分析

**TrapContext 的结构设计：**
```rust
#[repr(C)]
pub struct TrapContext {
    pub x: [usize; 32],
    pub sstatus: Sstatus,
    pub sepc: usize,
}

系统调用分发：
在 trap_handler 函数中，我们通过读取 scause 寄存器来判断 Trap 的原因：

Rust

match cause.cause() {
    Trap::Exception(Exception::UserEnvCall) => {
        // 1. 修改 sepc，使其指向 ecall 的下一条指令，避免死循环
        cx.sepc += 4;
        // 2. 将寄存器 a7（系统调用号）和 a0-a2（参数）分发给 syscall 函数
        cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
    }
    // ... 处理其他异常（如 StorePageFault, IllegalInstruction 等）
}
3.3 实验中需要填写的/完善的代码 (根据实际作业要求修改)
(提示：在这里写出本次 Lab 中要求你独立实现的练习题，例如实现 sys_task_info，或者完善某个系统调用)

练习1： 完善了 sys_write，支持将字符输出到终端。

练习2： 实现了 sys_exit，在当前任务结束后正确调用内核调度器切换到下一个任务。

4. 遇到并解决的问题
内联汇编导致上下文破坏的问题

问题描述：在最开始编写陷入汇编代码时，未正确对齐栈指针（sp），导致 TrapContext 写入到了非预期的内存位置。

解决过程：通过 GDB 单步调试以及查阅 RISC-V 汇编手册，确保在压栈前 sp 的地址是对齐的，并且按 8 字节的偏移量正确保存了所有的 x0-x31 寄存器。

Rust 所有权与生命周期机制带来的挑战

问题描述：在内核中尝试修改全局的任务管理器状态时，编译器报出了 cannot borrow as mutable 的错误。

解决过程：学习并引入了 UPSafeCell（或 spin::Mutex），通过内部可变性（Interior Mutability）在单核环境下安全地获取并修改全局静态变量。

5. 实验结果
通过执行 make run，内核能够成功编译并在 QEMU 中启动。系统成功加载了用户测例，所有的 Usertests 均通过（附部分核心截图或日志）。

Plaintext

[kernel] Hello, world!
[kernel] Application 0 loaded.
Hello, world from user mode program!
Test write A OK!
...
[kernel] All applications completed!

6. 实验总结与体会
通过本次实验，我真正从底层理解了“操作系统是如何接管硬件的”。以前在应用层编程时，系统调用只是一个黑盒函数，而现在我不仅亲自编写了触发系统调用的 ecall，还完整走通了保存上下文 -> 陷入内核 -> 查表分发系统调用 -> 恢复上下文 -> 返回用户态的闭环。

在此过程中，Rust 语言严格的编译器虽然带来了不少初期的麻烦，但也极大地避免了 C 语言中常见的内存越界和并发数据竞争问题，让我体会到了现代系统级编程语言的魅力。1
