.pushsection .text.memo_remote,"ax"
.balign 4
.global memo_remote_start
memo_remote_start:
    addi sp, sp, -256
    li a0, 0
    mv a1, sp
    li a2, 1
    li a7, {collect}
    ebreak
    li t0, 1
    bne a0, t0, 9f
    sd t0, 128(sp)
    ld t1, 0(sp)
    sd t1, 136(sp)
    sd zero, 144(sp)
    sd zero, 152(sp)
    sd zero, 160(sp)
    sd zero, 168(sp)
1:
    addi a0, sp, 128
    addi a1, sp, 192
    li a7, {inspect}
    ebreak
    bnez a0, 9f
    ld t0, 192(sp)
    sd t0, 168(sp)
    ld t0, 200(sp)
    li t1, 1
    beq t0, t1, 3f
    addi a0, sp, 128
    li a7, {remove}
    ebreak
    li t0, -1
    bne a0, t0, 9f
    addi a0, sp, 128
    li a7, {retract}
    ebreak
    beqz a0, 3f
    li t0, -7
    bne a0, t0, 9f
    li a7, {yield_}
    ebreak
    j 1b
3:
    addi a0, sp, 128
    li a7, {publish}
    ebreak
    li t0, -1
    bne a0, t0, 9f
    li t0, 1
    sd t0, 160(sp)
    addi a0, sp, 128
    addi a1, sp, 192
    li a7, {inspect}
    ebreak
    li t0, -4
    bne a0, t0, 9f
    li a0, 136
    j 10f
9:
    li a0, 1
10:
    li a1, 0
    li a2, 0
    li a3, 0
    li a4, 0
    li a7, {exit}
    ebreak
    j .
.global memo_remote_end
memo_remote_end:
.popsection
