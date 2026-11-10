use crate::boot::ledger::ResourceKind;
use crate::boot::ledger::{Name, Page};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    name: Name,
    kind: ResourceKind,
    handle: usize,
    view: Option<(crate::model::view::ViewKind, usize)>,
}
pub const ENTRY_LEN: usize = Entry::LEN;
const _: () = assert!(ENTRY_LEN == 40);
impl Entry {
    /// Encoded ledger entry size; independent of the Rust struct layout.
    pub const LEN: usize = 40;
    pub const NONE: Self = Self::new(Name::Page(Page::Dtb), ResourceKind::Share, 0);
    pub const fn new(name: Name, kind: ResourceKind, handle: usize) -> Self {
        Self {
            name,
            kind,
            handle,
            view: None,
        }
    }
    /// A data-flow View and its control-flow Sign, each independently authorized.
    pub const fn pair(
        name: Name,
        kind: crate::model::view::ViewKind,
        view: usize,
        sign: usize,
    ) -> Self {
        Self {
            name,
            kind: ResourceKind::Sign,
            handle: sign,
            view: Some((kind, view)),
        }
    }
    pub const fn dispatch(&self) -> Option<((crate::model::view::ViewKind, usize), usize)> {
        match self.view {
            Some(view) => Some((view, self.handle)),
            None => None,
        }
    }
    pub const fn name(&self) -> Name {
        self.name
    }
    pub const fn kind(&self) -> ResourceKind {
        self.kind
    }
    pub const fn handle(&self) -> usize {
        self.handle
    }
    /// Write one entry at `at`, returning the first byte after it.
    pub fn store_at(&self, out: &mut [u8], at: usize) -> Option<usize> {
        let end = at.checked_add(Self::LEN)?;
        let out = out.get_mut(at..end)?;
        self.name.store(&mut out[..16]);
        out[16] = self.kind as u8;
        out[17..24].fill(0);
        out[32..40].fill(0);
        if let Some((kind, handle)) = self.view {
            out[17] = match kind {
                crate::model::view::ViewKind::Clone => 1,
                crate::model::view::ViewKind::Share => 2,
            };
            out[32..40].copy_from_slice(&(handle as u64).to_le_bytes());
        }
        out[24..32].copy_from_slice(&(self.handle as u64).to_le_bytes());
        Some(end)
    }
    /// Read one entry, validating its field encodings and reserved bytes.
    /// Ledger validity (including duplicate names) is checked by `Entries::new`.
    pub fn fetch_at(bytes: &[u8], at: usize) -> Option<(Self, usize)> {
        let end = at.checked_add(Self::LEN)?;
        let bytes = bytes.get(at..end)?;
        if bytes[18..24].iter().any(|byte| *byte != 0) {
            return None;
        }
        let name = Name::fetch(&bytes[..16])?;
        let kind = ResourceKind::of(bytes[16])?;
        let handle = usize::try_from(u64::from_le_bytes(bytes[24..32].try_into().ok()?)).ok()?;
        let view_handle =
            usize::try_from(u64::from_le_bytes(bytes[32..40].try_into().ok()?)).ok()?;
        let view = match (bytes[17], view_handle) {
            (0, 0) => None,
            (1, n) if n != 0 => Some((crate::model::view::ViewKind::Clone, n)),
            (2, n) if n != 0 => Some((crate::model::view::ViewKind::Share, n)),
            _ => return None,
        };
        Some((
            Self {
                name,
                kind,
                handle,
                view,
            },
            end,
        ))
    }
    pub fn valid(&self) -> bool {
        self.handle != 0
            && match (self.name, self.view) {
                (
                    Name::Interrupt { .. } | Name::Memory,
                    Some((crate::model::view::ViewKind::Share, view)),
                ) => view != 0 && view != self.handle,
                (Name::Interrupt { .. } | Name::Memory, _) => false,
                (_, None) => true,
                _ => false,
            }
            && self.kind == self.name.kind()
            && !matches!(
                self.name,
                Name::Page(Page::Region(0)) | Name::Nest { controller: 0, .. }
            )
    }
}
pub const VERSION: u32 = 9;
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub version: u32,
    pub count: u32,
}

impl Header {
    pub const LEN: usize = 8;

    pub fn store_at(&self, out: &mut [u8], at: usize) -> Option<usize> {
        let end = at.checked_add(Self::LEN)?;
        let out = out.get_mut(at..end)?;
        out[..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..8].copy_from_slice(&self.count.to_le_bytes());
        Some(end)
    }

    pub fn fetch_at(bytes: &[u8], at: usize) -> Option<(Self, usize)> {
        let end = at.checked_add(Self::LEN)?;
        let bytes = bytes.get(at..end)?;
        Some((
            Self {
                version: u32::from_le_bytes(bytes[..4].try_into().ok()?),
                count: u32::from_le_bytes(bytes[4..8].try_into().ok()?),
            },
            end,
        ))
    }
}

/// Encode the startup ledger and return its used length, leaving spare capacity
/// untouched. Insufficient capacity leaves the entire output untouched.
/// This writes records; `Entries::new` validates the complete ledger's contents.
pub fn store_entries(out: &mut [u8], entries: &[Entry]) -> Option<usize> {
    let count = u32::try_from(entries.len()).ok()?;
    let len = Header::LEN.checked_add(entries.len().checked_mul(ENTRY_LEN)?)?;
    let out = out.get_mut(..len)?;
    let mut at = Header {
        version: VERSION,
        count,
    }
    .store_at(out, 0)?;
    for entry in entries {
        at = entry.store_at(out, at)?;
    }
    Some(at)
}

/// A validated complete startup ledger, with no trailing bytes.
pub struct Entries<'a> {
    bytes: &'a [u8],
    count: usize,
}
impl<'a> Entries<'a> {
    pub fn new(bytes: &'a [u8]) -> Option<Self> {
        let (header, at) = Header::fetch_at(bytes, 0)?;
        let count = usize::try_from(header.count).ok()?;
        let end = at.checked_add(count.checked_mul(ENTRY_LEN)?)?;
        if header.version != VERSION || end != bytes.len() {
            return None;
        }
        let entries = Self {
            bytes: bytes.get(at..end)?,
            count,
        };
        for i in 0..count {
            let entry = Entry::fetch_at(entries.bytes, i * ENTRY_LEN)?.0;
            if !entry.valid()
                || entries
                    .iter()
                    .take(i)
                    .any(|prior| prior.name() == entry.name())
            {
                return None;
            }
        }
        Some(entries)
    }
    pub fn iter(&self) -> impl Iterator<Item = Entry> + '_ {
        (0..self.count).filter_map(|i| Entry::fetch_at(self.bytes, i * ENTRY_LEN).map(|x| x.0))
    }
    pub fn find(&self, name: Name) -> Option<Entry> {
        self.iter().find(|entry| entry.name() == name)
    }
}
