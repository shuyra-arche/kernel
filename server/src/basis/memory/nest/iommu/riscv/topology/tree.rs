use super::RouteError;
use fdt::{Fdt, node::FdtNode};

pub(super) fn scalar(node: FdtNode, name: &str) -> Result<Option<u32>, RouteError> {
    node.property(name)
        .map(|property| number(property.value))
        .transpose()
}

pub(super) fn number(value: &[u8]) -> Result<u32, RouteError> {
    Ok(u32::from_be_bytes(
        value.try_into().map_err(|_| RouteError::Malformed)?,
    ))
}

pub(super) fn parent<'b, 'a>(fdt: &'b Fdt<'a>, node: FdtNode<'b, 'a>) -> Option<FdtNode<'b, 'a>> {
    fdt.all_nodes()
        .find(|p| p.children().any(|c| core::ptr::eq(c.name, node.name)))
}

/// One physical reg range on a root or explicitly identity-translated bus.
pub(super) fn physical(fdt: &Fdt, node: FdtNode) -> Result<(usize, usize), RouteError> {
    let mut bus = parent(fdt, node).ok_or(RouteError::Malformed)?;
    let addresses = scalar(bus, "#address-cells")?.unwrap_or(2) as usize;
    let sizes = scalar(bus, "#size-cells")?.unwrap_or(1) as usize;
    if !(1..=2).contains(&addresses) || !(1..=2).contains(&sizes) {
        return Err(RouteError::Unsupported);
    }
    while let Some(p) = parent(fdt, bus) {
        if !bus.property("ranges").is_some_and(|r| r.value.is_empty()) {
            return Err(RouteError::Unsupported);
        }
        bus = p;
    }
    let reg = node.property("reg").ok_or(RouteError::Malformed)?.value;
    if reg.len() != (addresses + sizes) * 4 {
        return Err(RouteError::Malformed);
    }
    let decode = |bytes: &[u8]| {
        bytes.iter().try_fold(0usize, |value, byte| {
            value
                .checked_mul(256)
                .and_then(|v| v.checked_add(*byte as usize))
                .ok_or(RouteError::Malformed)
        })
    };
    let base = decode(&reg[..addresses * 4])?;
    let size = decode(&reg[addresses * 4..])?;
    if size == 0 || base.checked_add(size).is_none() {
        return Err(RouteError::Malformed);
    }
    Ok((base, size))
}
