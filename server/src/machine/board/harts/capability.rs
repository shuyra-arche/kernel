//! Retain only capabilities consumed by this Kernel, not an ISA inventory.
use super::{
    HartError,
    property::{property, string},
};
use fdt::node::FdtNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Paging {
    Sv39,
    Sv48,
    Sv57,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Capability {
    pub paging: Paging,
    isa: u16,
}

// riscv64gc code generation plus explicit CSR, fence.i and rdtime consumers.
const REQUIRED: u16 = (1 << 9) - 1;
fn extension(name: &str) -> u16 {
    match name {
        "i" => 1,
        "m" => 2,
        "a" => 4,
        "f" => 8,
        "d" => 16,
        "c" => 32,
        "zicsr" => 64,
        "zifencei" => 128,
        "zicntr" => 256,
        _ => 0,
    }
}

impl Capability {
    pub(super) fn ceiling() -> Self {
        Self {
            paging: Paging::Sv57,
            isa: REQUIRED,
        }
    }
    pub(super) fn intersect(&mut self, other: Self) {
        self.paging = self.paging.min(other.paging);
        self.isa &= other.isa;
    }
    pub(super) fn parse(node: FdtNode<'_, '_>, id: usize) -> Result<Self, HartError> {
        let paging = match string(node, "mmu-type")? {
            "riscv,sv39" => Paging::Sv39,
            "riscv,sv48" => Paging::Sv48,
            "riscv,sv57" => Paging::Sv57,
            _ => return Err(HartError::Mmu(id)),
        };
        let base = property(node, "riscv,isa-base")?;
        let extensions = property(node, "riscv,isa-extensions")?;
        let isa = match (base, extensions) {
            (Some(_), Some(values)) => {
                if string(node, "riscv,isa-base")? != "rv64i" {
                    return Err(HartError::Isa(id));
                }
                modern(values)?
            }
            (None, None) => legacy(string(node, "riscv,isa")?)?,
            _ => return Err(HartError::Property("ISA property pair")),
        };
        if isa & REQUIRED != REQUIRED {
            return Err(HartError::Isa(id));
        }
        Ok(Self { paging, isa })
    }
}

fn modern(values: &[u8]) -> Result<u16, HartError> {
    let fail = HartError::Property("riscv,isa-extensions");
    let values = values.strip_suffix(&[0]).ok_or(fail)?;
    if values.len() > 4096 {
        return Err(fail);
    }
    let mut isa = 0;
    for (index, bytes) in values.split(|b| *b == 0).enumerate() {
        let name = core::str::from_utf8(bytes).map_err(|_| fail)?;
        if !valid_extension(name) || values.split(|b| *b == 0).take(index).any(|v| v == bytes) {
            return Err(fail);
        }
        isa |= extension(name);
    }
    Ok(isa)
}

fn valid_extension(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn legacy(value: &str) -> Result<u16, HartError> {
    let fail = HartError::Property("riscv,isa");
    if value.len() > 4096 {
        return Err(fail);
    }
    let rest = value.strip_prefix("rv64i").ok_or(fail)?;
    // The deprecated DT binding predates the CSR/fence/counter split: I implies
    // Zicsr, Zifencei and Zicntr. Modern extension arrays must name them explicitly.
    let mut isa = 1 | 64 | 128 | 256;
    let mut parts = rest.split('_');
    let singles = parts.next().unwrap();
    let order = b"mafdqcbkjpvh";
    let mut next = 0;
    for b in singles.bytes() {
        let offset = order[next..].iter().position(|v| *v == b).ok_or(fail)?;
        next += offset + 1;
        isa |= match b {
            b'm' => 2,
            b'a' => 4,
            b'f' => 8,
            b'd' => 16,
            b'c' => 32,
            _ => 0,
        };
    }
    for name in parts {
        if name.len() < 2
            || !matches!(name.as_bytes()[0], b's' | b'h' | b'z' | b'x')
            || !valid_extension(name)
        {
            return Err(fail);
        }
        isa |= extension(name);
    }
    Ok(isa)
}
