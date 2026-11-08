.section .text._start
.globl _start
_start:
    la   t0, PER_HART
    slli t1, a0, 6
    add  tp, t0, t1
    csrc sstatus, 2
    la   sp, _kernel_edge
    ld   t0, _canary
    sd   t0, 0(sp)
    la   t0, _stack
    ld   t0, 0(t0)
    add  sp, sp, t0
    tail main
