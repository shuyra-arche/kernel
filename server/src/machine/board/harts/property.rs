use super::HartError;
use fdt::node::FdtNode;

pub(super) fn property<'a>(
    node: FdtNode<'_, 'a>,
    name: &'static str,
) -> Result<Option<&'a [u8]>, HartError> {
    let mut values = node.properties().filter(|p| p.name == name);
    let value = values.next().map(|p| p.value);
    if values.next().is_some() {
        return Err(HartError::Property(name));
    }
    Ok(value)
}

pub(super) fn string<'a>(node: FdtNode<'_, 'a>, name: &'static str) -> Result<&'a str, HartError> {
    let value = property(node, name)?.ok_or(HartError::Property(name))?;
    let bytes = value
        .strip_suffix(&[0])
        .filter(|v| !v.contains(&0))
        .ok_or(HartError::Property(name))?;
    core::str::from_utf8(bytes).map_err(|_| HartError::Property(name))
}

pub(super) fn number(node: FdtNode<'_, '_>, name: &'static str) -> Result<usize, HartError> {
    let value = property(node, name)?.ok_or(HartError::Property(name))?;
    match value.len() {
        4 => Ok(u32::from_be_bytes(value.try_into().unwrap()) as usize),
        8 if name == "reg" || name == "timebase-frequency" => {
            usize::try_from(u64::from_be_bytes(value.try_into().unwrap()))
                .map_err(|_| HartError::Property(name))
        }
        _ => Err(HartError::Property(name)),
    }
}

pub(super) fn enabled(node: FdtNode<'_, '_>) -> Result<bool, HartError> {
    if property(node, "status")?.is_none() {
        return Ok(true);
    }
    match string(node, "status")? {
        "ok" | "okay" => Ok(true),
        "disabled" | "reserved" | "fail" => Ok(false),
        value if value.starts_with("fail-") => Ok(false),
        _ => Err(HartError::Property("status")),
    }
}
