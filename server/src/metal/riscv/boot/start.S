.section .text._start
.globl _start
_start:
    # The boot hart owns dense slot zero, regardless of firmware ID.
    la   tp, PER_HART
    sd   a0, 0x28(tp)
    csrc sstatus, 2
    la   sp, _kernel_edge
    ld   t0, _canary
    sd   t0, 0(sp)
    la   t0, _stack
    ld   t0, 0(t0)
    add  sp, sp, t0
    tail main
