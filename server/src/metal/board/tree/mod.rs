//! Shared interpretation of firmware properties used by boot authority and
//! hardware routing. Identity indexes borrow the same immutable DTB; they do
//! not confer authority or perform hardware effects.
pub(crate) mod mmio;
use alloc::vec::Vec;
use fdt::{Fdt, node::FdtNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    Description,
    OutOfMemory,
}

pub(crate) fn property<'a>(node: FdtNode<'_, 'a>, name: &str) -> Result<Option<&'a [u8]>, Error> {
    let mut values = node.properties().filter(|p| p.name == name);
    let value = values.next().map(|p| p.value);
    if values.next().is_some() {
        return Err(Error::Description);
    }
    Ok(value)
}

pub(crate) fn number(bytes: &[u8]) -> Result<usize, Error> {
    if !matches!(bytes.len(), 4 | 8) {
        return Err(Error::Description);
    }
    bytes.iter().try_fold(0usize, |n, b| {
        n.checked_mul(256)
            .and_then(|n| n.checked_add(*b as usize))
            .ok_or(Error::Description)
    })
}

pub(crate) fn cells(node: FdtNode<'_, '_>, name: &str, default: usize) -> Result<usize, Error> {
    match property(node, name)? {
        None => Ok(default),
        Some(bytes) if bytes.len() == 4 => number(bytes),
        _ => Err(Error::Description),
    }
}

pub(crate) fn parent<'b, 'a>(fdt: &'b Fdt<'a>, node: FdtNode<'b, 'a>) -> Option<FdtNode<'b, 'a>> {
    // Names have distinct storage even when equal under different parents.
    fdt.all_nodes()
        .find(|p| p.children().any(|c| core::ptr::eq(c.name, node.name)))
}

pub(crate) fn enabled<'b, 'a>(fdt: &'b Fdt<'a>, mut node: FdtNode<'b, 'a>) -> Result<bool, Error> {
    loop {
        match property(node, "status")? {
            None | Some(b"okay\0" | b"ok\0") => {}
            Some(b"disabled\0" | b"reserved\0" | b"fail\0") => return Ok(false),
            Some(s)
                if s.starts_with(b"fail-")
                    && s.ends_with(&[0])
                    && !s[..s.len() - 1].contains(&0) =>
            {
                return Ok(false);
            }
            _ => return Err(Error::Description),
        }
        match parent(fdt, node) {
            Some(p) => node = p,
            None => return Ok(true),
        }
    }
}

pub(crate) fn scalar(node: FdtNode, name: &str) -> Result<Option<u32>, Error> {
    property(node, name)?.map(word).transpose()
}

pub(crate) fn word(value: &[u8]) -> Result<u32, Error> {
    Ok(u32::from_be_bytes(
        value.try_into().map_err(|_| Error::Description)?,
    ))
}

pub(crate) fn phandle(node: FdtNode) -> Result<Option<u32>, Error> {
    let a = scalar(node, "phandle")?;
    let b = scalar(node, "linux,phandle")?;
    if a.is_some() && b.is_some() && a != b {
        return Err(Error::Description);
    }
    let id = a.or(b);
    if matches!(id, Some(0 | u32::MAX)) {
        return Err(Error::Description);
    }
    Ok(id)
}

/// A phandle is tree-wide. Never resolve an ambiguous identity by first match.
pub(crate) struct Identities<'b, 'a> {
    nodes: Vec<(u32, FdtNode<'b, 'a>)>,
}
impl<'b, 'a> Identities<'b, 'a> {
    pub(crate) fn discover(tree: &'b Fdt<'a>) -> Result<Self, Error> {
        let mut result = Self { nodes: Vec::new() };
        for node in tree.all_nodes() {
            if let Some(id) = phandle(node)? {
                if result.resolve(id).is_some() {
                    return Err(Error::Description);
                }
                result
                    .nodes
                    .try_reserve(1)
                    .map_err(|_| Error::OutOfMemory)?;
                result.nodes.push((id, node));
            }
        }
        Ok(result)
    }
    pub(crate) fn resolve(&self, id: u32) -> Option<FdtNode<'b, 'a>> {
        self.nodes
            .iter()
            .find_map(|(key, node)| (*key == id).then_some(*node))
    }
    pub(crate) fn id(&self, node: FdtNode) -> Option<u32> {
        self.nodes
            .iter()
            .find_map(|(key, stored)| core::ptr::eq(stored.name, node.name).then_some(*key))
    }
}
