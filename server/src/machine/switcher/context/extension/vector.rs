use alloc::vec::Vec;

/// A baseline V 1.0 profile. Retain the complete supported SEW set, so a
/// destination with different element capabilities cannot inherit an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    bytes: usize,
    widths: u8,
}
impl Shape {
    pub(crate) fn checked(bytes: usize, widths: u8) -> Option<Self> {
        (bytes.is_power_of_two() && (16..=8192).contains(&bytes) && widths == 0x0f)
            .then_some(Self { bytes, widths })
    }
}

pub(super) struct Vector {
    shape: Shape,
    // u64 provides the largest supported EEW alignment; length is 32*vlenb.
    registers: Vec<u64>,
    control: [usize; 4], // vstart, vl, vtype, vcsr
}
impl Vector {
    pub(super) fn new(shape: Shape) -> Result<Self, ()> {
        let mut registers = Vec::new();
        let words = shape.bytes.checked_mul(4).ok_or(())?;
        registers.try_reserve_exact(words).map_err(|_| ())?;
        registers.resize(words, 0);
        Ok(Self {
            shape,
            registers,
            control: [0, 0, 1usize << 63, 0],
        })
    }
    pub(super) fn shape(&self) -> Shape {
        self.shape
    }
    #[cfg(target_arch = "riscv64")]
    pub(super) unsafe fn capture(&mut self) {
        unsafe {
            save(
                self.registers.as_mut_ptr(),
                self.control.as_mut_ptr(),
                self.shape.bytes,
            )
        };
    }
    #[cfg(target_arch = "riscv64")]
    pub(super) unsafe fn restore(&self) {
        unsafe {
            load(
                self.registers.as_ptr(),
                self.control.as_ptr(),
                self.shape.bytes,
            )
        };
    }
}

#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
unsafe extern "C" fn save(_data: *mut u64, _control: *mut usize, _bytes: usize) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +v",
        "li t0, 0x600",
        "csrs sstatus, t0",
        "csrr t1, vstart",
        "sd t1, 0(a1)",
        "csrr t1, vl",
        "sd t1, 8(a1)",
        "csrr t1, vtype",
        "sd t1, 16(a1)",
        "csrr t1, vcsr",
        "sd t1, 24(a1)",
        // Whole-register operations also honor vstart: clear it BEFORE saving.
        "csrw vstart, zero",
        "slli a2, a2, 3",
        "vs8r.v v0, (a0)",
        "add a0, a0, a2",
        "vs8r.v v8, (a0)",
        "add a0, a0, a2",
        "vs8r.v v16, (a0)",
        "add a0, a0, a2",
        "vs8r.v v24, (a0)",
        "csrc sstatus, t0",
        "ret",
        ".option pop",
    );
}
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
unsafe extern "C" fn load(_data: *const u64, _control: *const usize, _bytes: usize) {
    core::arch::naked_asm!(
        ".option push",
        ".option arch, +v",
        "li t0, 0x600",
        "csrs sstatus, t0",
        "csrw vstart, zero",
        "slli a2, a2, 3",
        "vl8re8.v v0, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v8, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v16, (a0)",
        "add a0, a0, a2",
        "vl8re8.v v24, (a0)",
        "ld t1, 8(a1)",
        "ld t2, 16(a1)",
        "vsetvl zero, t1, t2",
        "ld t1, 24(a1)",
        "csrw vcsr, t1",
        // Configuration and loads reset vstart; restore it LAST, without a
        // subsequent vector instruction before the Task resumes.
        "ld t1, 0(a1)",
        "csrw vstart, t1",
        "csrc sstatus, t0",
        "ret",
        ".option pop",
    );
}
