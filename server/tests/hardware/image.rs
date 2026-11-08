//! Minimal ELF-to-Capsule assembly for the actual Kernel guest. This emits the
//! public boot contract; it does not bypass Kernel mapping or task construction.
use kernel_api::boot::capsule::{Capsule, HEADER, MAGIC, PAGE, RECORD};
fn u16_at(bytes: &[u8], at: usize) -> usize {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize
}
fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn u64_at(bytes: &[u8], at: usize) -> usize {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize
}
fn put(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 8].copy_from_slice(&(value as u64).to_le_bytes());
}
pub(super) fn initrd(elf: &[u8]) -> Vec<u8> {
    assert_eq!(&elf[..6], b"\x7fELF\x02\x01");
    let entry = u64_at(elf, 24);
    let table = u64_at(elf, 32);
    let width = u16_at(elf, 54);
    let mut regions = Vec::new();
    let mut capsule = vec![0; PAGE];
    for i in 0..u16_at(elf, 56) {
        let at = table + i * width;
        let size = u64_at(elf, at + 40);
        if u32_at(elf, at) != 1 || size == 0 {
            continue;
        }
        let flags = u32_at(elf, at + 4);
        let offset = u64_at(elf, at + 8);
        let va = u64_at(elf, at + 16);
        let bytes = u64_at(elf, at + 32);
        assert!(va.is_multiple_of(PAGE));
        let pages = size.div_ceil(PAGE);
        let data = bytes.div_ceil(PAGE);
        let payload = if data == 0 { 0 } else { capsule.len() };
        if data != 0 {
            capsule.extend_from_slice(&elf[offset..offset + bytes]);
            capsule.resize(payload + data * PAGE, 0);
        }
        regions.push([
            va,
            pages,
            data,
            payload,
            1 | if flags & 2 != 0 { 2 } else { 0 } | if flags & 1 != 0 { 4 } else { 0 },
        ]);
    }
    assert!(HEADER + regions.len() * RECORD <= PAGE);
    capsule[..8].copy_from_slice(&MAGIC);
    capsule[8..12].copy_from_slice(&[2, 0, 12, kernel_api::abi::VERSION as u8]);
    capsule[12..16].copy_from_slice(&(regions.len() as u32).to_le_bytes());
    put(&mut capsule, 16, entry);
    let length = capsule.len();
    put(&mut capsule, 24, length);
    for (i, region) in regions.iter().enumerate() {
        for (word, value) in region.iter().enumerate() {
            put(&mut capsule, HEADER + i * RECORD + word * 8, *value);
        }
    }
    assert!(Capsule::parse(&capsule).is_some());
    let mut image = vec![0; PAGE];
    image[..4].copy_from_slice(&(PAGE as u32).to_le_bytes());
    image[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    image.extend_from_slice(&capsule);
    image
}
