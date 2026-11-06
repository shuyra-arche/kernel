//! The Kernel reads only an offset and length (two little-endian u32 words).
//! Bytes after this preamble are owned by the deployment format.

pub const PREAMBLE: usize = 8;

/// Locate the initial capsule without interpreting the deployment manifest.
pub fn entry_image(blob: &[u8]) -> Option<&[u8]> {
    let header = blob.get(..PREAMBLE)?;
    let off = u32::from_le_bytes(header[..4].try_into().ok()?) as usize;
    let len = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
    blob.get(off..off.checked_add(len)?)
}
