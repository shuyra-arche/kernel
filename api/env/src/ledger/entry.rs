use crate::{Name, Page, PieKind, PieToken};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    name: Name,
    kind: PieKind,
    token: PieToken,
}
pub const ENTRY_LEN: usize = Entry::LEN;
const _: () = assert!(ENTRY_LEN == 32);
impl Entry {
    /// Encoded ledger entry size; independent of the Rust struct layout.
    pub const LEN: usize = 32;
    pub const NONE: Self = Self::new(Name::Page(Page::Dtb), PieKind::Pole, PieToken::NONE);
    pub const fn new(name: Name, kind: PieKind, token: PieToken) -> Self {
        Self { name, kind, token }
    }
    pub const fn name(&self) -> Name {
        self.name
    }
    pub const fn kind(&self) -> PieKind {
        self.kind
    }
    pub const fn token(&self) -> PieToken {
        self.token
    }
    /// Write one entry at `at`, returning the first byte after it.
    pub fn store_at(&self, out: &mut [u8], at: usize) -> Option<usize> {
        let end = at.checked_add(Self::LEN)?;
        let out = out.get_mut(at..end)?;
        self.name.store(&mut out[..16]);
        out[16] = self.kind as u8;
        out[17..24].fill(0);
        out[24..32].copy_from_slice(&self.token.to_bytes());
        Some(end)
    }
    /// Read one entry, validating its field encodings and reserved bytes.
    /// Ledger validity (including duplicate names) is checked by `Entries::new`.
    pub fn fetch_at(bytes: &[u8], at: usize) -> Option<(Self, usize)> {
        let end = at.checked_add(Self::LEN)?;
        let bytes = bytes.get(at..end)?;
        if bytes[17..24].iter().any(|byte| *byte != 0) {
            return None;
        }
        Some((
            Self::new(
                Name::fetch(&bytes[..16])?,
                PieKind::of(bytes[16])?,
                PieToken::from_bytes(&bytes[24..32])?,
            ),
            end,
        ))
    }
    pub fn valid(&self) -> bool {
        self.token != PieToken::NONE
            && self.kind == self.name.kind()
            && !matches!(self.name, Name::Page(Page::Region(0)))
    }
}
pub const VERSION: u32 = 1;
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
