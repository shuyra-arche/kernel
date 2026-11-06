//! 一份引用上的子范围：句柄 + 相对页偏移 + 页数。
//!
//! 页数非零且坐标不溢出（见 [`pages`](super::pages)）；这是 `Attach` 长度的
//! 唯一来源，选址不另带一份长度。

use crate::model::memory::FlatReference;

use super::pages;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceRange {
    pub reference: FlatReference,
    pub offset: usize,
    pub count: usize,
}

impl ReferenceRange {
    pub fn new(reference: FlatReference, offset: usize, count: usize) -> Option<Self> {
        pages(offset, count)?;
        Some(Self {
            reference,
            offset,
            count,
        })
    }
}
