// One position-independent U-mode page. Grants are collected before admission;
// after the gate opens all execution uses the admitted call subset.
.option push
.option norvc
.pushsection .text.realtime_worker,"ax"
.balign 4
.global realtime_worker_start
realtime_worker_start:
    ld s0, 0(a0)                // role
    ld s1, 8(a0)                // timeout, ns (zero is infinite)
    addi s10, a0, 16            // encoded mapping preparation request
    addi sp, sp, -1024
    sd zero, 0(sp)
    sd zero, 8(sp)
    sd zero, 16(sp)
    sd zero, 24(sp)
    sd zero, 32(sp)
    sd zero, 40(sp)
    sd zero, 48(sp)
    li a0, 0
    addi a1, sp, 128
    li a2, 7
    li a7, {collect}
    ebreak
    bltz a0, worker_fail
    mv t0, a0
    addi t1, sp, 128
1:
    beqz t0, 2f
    ld t2, 40(t1)              // PendInfo.mark, not token order
    addi t2, t2, -1
    li t3, 7
    bgeu t2, t3, worker_fail
    slli t2, t2, 3
    add t2, sp, t2
    ld t3, 0(t1)
    sd t3, 0(t2)
    addi t1, t1, {pend_size}
    addi t0, t0, -1
    j 1b
2:
    ld s9, 40(sp)             // only the compute worker owns the data grant
    beqz s9, 11f
    ld a0, 48(sp)             // explicit Whole RESERVE grant
    mv a1, s10
    li a7, {reserve}
    ebreak
    bnez a0, worker_fail
    li t0, 1
    sd t0, 128(sp)            // source: Share Pend, relative page, generation zero
    sd s9, 136(sp)
    sd zero, 144(sp)
    sd zero, 152(sp)
    sd t0, 160(sp)            // target: mapping Pend, relative page, exact generation
    sd a1, 168(sp)
    sd zero, 176(sp)
    sd zero, 184(sp)
    addi a0, sp, 160
    addi a1, sp, 192
    li a7, {inspect}
    ebreak
    bnez a0, worker_fail
    ld t0, 192(sp)
    sd t0, 184(sp)
    addi a0, sp, 128
    li a7, {insert}
    ebreak
    bnez a0, worker_fail
    addi a0, sp, 160
    addi a1, sp, 192
    li a7, {inspect}
    ebreak
    bnez a0, worker_fail
    ld t0, 192(sp)
    sd t0, 184(sp)
    addi a0, sp, 160
    li a7, {publish}
    ebreak
    bnez a0, worker_fail
    li t0, 0x41000
    sd zero, 0(t0)            // materialize while still ordinary
11:
    ld s2, 0(sp)              // prepared
    ld s3, 8(sp)              // start/stop gate
    ld s4, 16(sp)             // service to await
    ld s5, 24(sp)             // service reply to publish
    ld s6, 32(sp)             // completion notification
    mv a0, s2
    li a7, {trigger}
    ebreak
    bnez a0, worker_fail
3:
    mv a0, s3
    li a1, 0
    li a2, 0
    li a7, {listen}
    ebreak
    beqz a0, 3b
    li t0, 9
    bne a0, t0, worker_fail
    mv a0, s3
    li a7, {consume}
    ebreak
    bnez a0, worker_fail
.global realtime_worker_active
realtime_worker_active:
    nop
    li t0, 6
    beq s0, t0, worker_listen
    li t0, 7
    beq s0, t0, worker_compute
    li t0, 2
    beq s0, t0, worker_await
    li t0, 3
    beq s0, t0, worker_restrict
    li t0, 4
    beq s0, t0, worker_fault
    li t0, 5
    beq s0, t0, worker_float
    // Acknowledge role 1 only after its first real admitted Yield.
    li t0, 1
    bne s0, t0, worker_spin
    li a7, {yield_}
    ebreak
    mv a0, s6
    li a7, {trigger}
    ebreak
    bnez a0, worker_fail
    // CPU-bound polling worker. Role 1 also yields repeatedly.
worker_spin:
    li t0, 10000
4:
    addi t0, t0, -1
    bnez t0, 4b
    li t0, 1
    bne s0, t0, 5f
    li a7, {yield_}
    ebreak
5:
    mv a0, s3
    li a1, 0
    li a2, 0
    li a7, {listen}
    ebreak
    beqz a0, worker_spin
    li t0, 9
    bne a0, t0, worker_fail
    li s8, 100
    j worker_reply
// No syscall occurs between these checkpoints. A remote Sign must preempt it.
worker_compute:
    mv a0, s6
    li a7, {trigger}
    ebreak
    bnez a0, worker_fail
.global realtime_compute_begin
realtime_compute_begin:
    nop
    li t0, 0x41000
10:
    ld t1, 0(t0)
    beqz t1, 10b
.global realtime_compute_end
realtime_compute_end:
    nop
    // Preserve the remaining budget after the preemption checkpoint while
    // the controller observes it and requests termination.
    mv a0, s3
    li a1, 2
    li a2, 0
    li a7, {listen}
    ebreak
    li t0, 9
    bne a0, t0, worker_fail
    li s8, 100
    j worker_reply
worker_listen:
    mv a0, s3
    li a1, 2
    li a2, 0
    li a7, {listen}
    ebreak
    li t0, 9
    bne a0, t0, worker_fail
.global realtime_preempted
realtime_preempted:
    nop
    li t0, 0x41000
    li t1, 1
    sd t1, 0(t0)
    li s8, 106
    j worker_reply
worker_await:
    li s7, 0
    beqz s1, 6f
    li a7, {clock}
    ebreak
    add s7, a0, s1
6:
    mv a0, s4
    li a1, 2
    beqz s1, 7f
    li a1, 1
7:
    mv a2, s7
    li a7, {await_}
    ebreak
    addi s8, a0, 200           // 200 timeout; 209 event; 199 Denied
    j worker_reply
worker_restrict:
    li a0, 0
    addi a1, sp, 128
    li a2, 1
    li a7, {collect}
    ebreak
    li t0, -1
    bne a0, t0, worker_fail
    sd zero, 64(sp)
    addi a0, sp, 64
    li a1, 1                  // mismatch would immediately return ordinarily
    li a2, 0
    li a3, 0
    li a7, {wait_value}
    ebreak
    li t0, -4
    bne a0, t0, worker_fail
    li a0, 0                  // RoomQuery::Ordinary
    li a7, {inspect_room}
    ebreak
    li t0, -4
    bne a0, t0, worker_fail
    li a7, {clock}
    ebreak
    li t0, 1000000
    add s7, a0, t0
    mv a0, s7
    li a7, {sleep}
    ebreak
    bnez a0, worker_fail
    li a7, {clock}
    ebreak
    bltu a0, s7, worker_fail
    li s8, 103
    j worker_reply
worker_fault:
    ld t0, 0(zero)            // must terminate, never materialize/delegate
    j worker_fail
worker_float:
    fmv.d.x f0, zero
    fadd.d f1, f0, f0
    fmv.x.d t0, f1
    bnez t0, worker_fail
    li s8, 105
worker_reply:
    beqz s5, 8f
    mv a0, s5
    li a7, {trigger}
    ebreak
    bnez a0, worker_fail
8:
    beqz s6, 9f
    mv a0, s6
    li a7, {trigger}
    ebreak
    bnez a0, worker_fail
9:
    mv a0, s8
    j worker_exit
worker_fail:
    li a0, 255
worker_exit:
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, {exit}
    ebreak
    j .
.global realtime_worker_end
realtime_worker_end:
.popsection
.option pop
