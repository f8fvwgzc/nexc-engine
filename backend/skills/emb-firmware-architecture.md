---
id: emb-firmware-architecture
name: Firmware architecture
category: embedded
description: Designs microcontroller firmware structure: bare metal or RTOS, task priorities and timing analysis, ISR rules, memory and stack budgets, low-power modes, and bootloader and field-update paths.
keywords: firmware, rtos, bare metal, freertos, zephyr, scheduling, interrupt, isr, stack, memory budget, low power, bootloader, mcuboot, ota, rate monotonic, watchdog
source: Zephyr Project documentation - Scheduling | https://docs.zephyrproject.org/latest/kernel/services/scheduling/index.html
source: MCUboot - Bootloader design | https://docs.mcuboot.com/design.html
source: FreeRTOS - Heap memory management | https://www.freertos.org/Documentation/02-Kernel/02-Kernel-features/09-Memory-management/01-Memory-management
---
## When to use
New MCU product, a superloop that misses deadlines, RTOS selection, RAM/flash/stack sizing, battery-life targets, or field-update design.

## Decision rules
- Superloop plus interrupts: few activities, simple timing, smallest footprint; worst-case loop time is the sum of all paths.
- Time-triggered (static table): deterministic and easy to argue for safety; every job must fit its slot.
- Preemptive RTOS (FreeRTOS, Zephyr, Eclipse ThreadX, a certified kernel for SIL/ASIL work): several rates and deadlines, blocking I/O, BLE/TCP/USB stacks. Costs: a stack per task, priority inversion, harder WCET.
- Embedded Linux: MMU, rich UI or networking; put hard real-time on a co-processor or use PREEMPT_RT (mainline since Linux 6.12).

## Scheduling
- Rate-monotonic priorities (shorter period = higher priority); deadline-monotonic when D < T.
- Utilisation U = sum(Ci/Ti). Liu-Layland sufficient bound: U <= n(2^(1/n) - 1) (0.780 for n = 3, about 0.69 for large n).
- Exact test, response time: Ri = Ci + Bi + sum over higher-priority j of ceil(Ri/Tj)*Cj; iterate from Ri = Ci until stable; schedulable if Ri <= Di. Ci = WCET, Bi = longest blocking by lower-priority critical sections.
- Shared resources: mutex with priority inheritance or ceiling; never a binary semaphore as a lock.
- Priority numbering differs: FreeRTOS higher number = higher priority; Zephyr negative = cooperative, 0 and up = preemptible, lower number = higher priority.

## ISR rules
- Acknowledge, capture, timestamp, then defer work to a task (queue, task notification, Zephyr work queue).
- No blocking calls, heap, or printf; only ISR-safe APIs (FreeRTOS ...FromISR); interrupts logically above configMAX_SYSCALL_INTERRUPT_PRIORITY call no kernel API (on Cortex-M a lower number is more urgent).
- volatile gives visibility, not atomicity: use atomics or short critical sections; measure interrupt latency and the longest interrupts-disabled window.

## Memory, stack and power budgets
- Budget table: flash (bootloader, two image slots, NV config, code, const) and RAM (.data/.bss, task and ISR stacks, heaps, DMA buffers, stack pools). Keep 20-30% margin at first release (team rule, not a standard).
- Prefer static allocation; any heap allocates at init only (FreeRTOS heap_4 coalesces, heap_5 spans regions).
- Stack size = static call-graph analysis (-fstack-usage, map file) plus measured high-water mark (stack painting, uxTaskGetStackHighWaterMark); add worst ISR nesting to the main stack; enable overflow checks and MPU guard regions.
- Average current I_avg = sum(I_mode * t_mode)/T_cycle; life (h) = capacity (mAh) * derating / I_avg (mA). Use tickless idle and the deepest sleep that meets wake latency and RAM retention; brown-out detection on.

## Boot and update
- Chain: immutable first stage -> verified bootloader (e.g. MCUboot) -> application; check hash and signature before every jump.
- MCUboot primary/secondary slots: swap-using-offset (preferred over swap-using-move), swap-using-move, swap-using-scratch (may be dropped), overwrite-only, direct-XIP, RAM-load. Install as a test swap; the app sets image-OK after self-test or the next reset reverts. Swap progress survives power loss.
- Anti-rollback via a hardware security counter; keep a recovery path (serial/USB DFU).
- Independent watchdog fed by a supervisor that checks every task's heartbeat, never from a timer ISR or idle hook.

## Output
Architecture note: constraints; task table (period, deadline, WCET, priority, stack); ISR table; memory map with margins; power budget; boot/update flow; risks.

## Pitfalls
Work inside ISRs; unbounded critical sections; guessed stack sizes; heap fragmentation; OTA never power-cut tested; debug port open in production.
