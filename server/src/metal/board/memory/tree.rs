use super::{MemoryError, Region};

/// Check the byte envelope before the infallible iterators in `fdt` see it.
/// The firmware must still provide a readable blob; a pointer cannot prove that.
pub(super) fn validate(bytes: &[u8]) -> Result<core::ops::Range<usize>, MemoryError> {
    let word = |at| read(bytes, at);
    if bytes.len() < 40
        || word(0)? != 0xd00dfeed
        || word(4)? != bytes.len()
        || word(20)? < 17
        || word(24)? > 17
    {
        return Err(MemoryError::Tree);
    }
    let structure = block(bytes, word(8)?, word(36)?)?;
    let strings = block(bytes, word(12)?, word(32)?)?;
    if !structure.start.is_multiple_of(4) || overlaps(&structure, &strings) {
        return Err(MemoryError::Tree);
    }
    let reserve_start = word(16)?;
    if reserve_start < 40 || !reserve_start.is_multiple_of(8) {
        return Err(MemoryError::Tree);
    }
    let mut end = reserve_start;
    loop {
        let entry = bytes
            .get(end..end.checked_add(16).ok_or(MemoryError::Tree)?)
            .ok_or(MemoryError::Tree)?;
        end += 16;
        if entry == [0; 16] {
            break;
        }
    }
    let reservations = reserve_start..end;
    if overlaps(&reservations, &structure) || overlaps(&reservations, &strings) {
        return Err(MemoryError::Tree);
    }
    let data = &bytes[structure];
    let names = &bytes[strings];
    let mut at = 0;
    let mut depth = 0;
    let mut root = false;
    let mut children = [false; 32];
    loop {
        let token = read(data, at)?;
        at += 4;
        match token {
            1 => {
                if depth == children.len() || (depth == 0 && root) {
                    return Err(MemoryError::Tree);
                }
                if depth > 0 {
                    children[depth - 1] = true;
                }
                children[depth] = false;
                let name = string(data.get(at..).ok_or(MemoryError::Tree)?)?;
                if depth == 0 && !name.is_empty() {
                    return Err(MemoryError::Tree);
                }
                at = (at + name.len() + 1).next_multiple_of(4);
                depth += 1;
                root = true;
            }
            2 => {
                depth = depth.checked_sub(1).ok_or(MemoryError::Tree)?;
            }
            3 => {
                if depth == 0 || children[depth - 1] {
                    return Err(MemoryError::Tree);
                }
                let size = read(data, at)?;
                let name = read(data, at + 4)?;
                string(names.get(name..).ok_or(MemoryError::Tree)?)?;
                at = at
                    .checked_add(8)
                    .and_then(|a| a.checked_add(size))
                    .and_then(|a| a.checked_next_multiple_of(4))
                    .ok_or(MemoryError::Tree)?;
                if at > data.len() {
                    return Err(MemoryError::Tree);
                }
            }
            4 => {}
            9 if root && depth == 0 => return Ok(reservations),
            _ => return Err(MemoryError::Tree),
        }
    }
}

pub(super) fn reservation(bytes: &[u8]) -> Result<Region, MemoryError> {
    let base = usize::try_from(u64::from_be_bytes(bytes[..8].try_into().unwrap()))
        .map_err(|_| MemoryError::Range)?;
    let size = usize::try_from(u64::from_be_bytes(bytes[8..].try_into().unwrap()))
        .map_err(|_| MemoryError::Range)?;
    Ok(Region::new(base, size))
}

fn read(bytes: &[u8], at: usize) -> Result<usize, MemoryError> {
    let value = bytes
        .get(at..at.checked_add(4).ok_or(MemoryError::Tree)?)
        .ok_or(MemoryError::Tree)?;
    Ok(u32::from_be_bytes(value.try_into().unwrap()) as usize)
}

fn block(bytes: &[u8], start: usize, size: usize) -> Result<core::ops::Range<usize>, MemoryError> {
    let end = start.checked_add(size).ok_or(MemoryError::Tree)?;
    if start < 40 || end > bytes.len() {
        return Err(MemoryError::Tree);
    }
    Ok(start..end)
}

fn overlaps(a: &core::ops::Range<usize>, b: &core::ops::Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn string(bytes: &[u8]) -> Result<&str, MemoryError> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or(MemoryError::Tree)?;
    core::str::from_utf8(&bytes[..end]).map_err(|_| MemoryError::Tree)
}
