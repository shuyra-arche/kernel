//! 同一个 Nest 的全部地址占用，按起点排序。固定与动态选址共用这份互斥记录。
//! 含尾区间允许占用延伸到 usize::MAX；退役中的区间继续占位，直到票结算。

use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use kernel_api::address::PAGE_SIZE;

use super::AddressError;

/// 一段占用；`last` 是最后一个字节（含），generation 是 `id`。
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    last: usize,
    ticket: usize,
    id: usize,
}

pub(crate) struct Occupancy {
    spans: Vec<Span>,
}

impl Occupancy {
    pub(super) const fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// 窗口内未被占用的区间（含端点），按地址升序。
    pub(super) fn free_spans(
        &self,
        base: usize,
        last: usize,
    ) -> Result<Vec<(usize, usize)>, AddressError> {
        let mut spans = Vec::new();
        spans
            .try_reserve(self.spans.len().saturating_add(1))
            .map_err(|_| AddressError::OutOfMemory)?;
        let mut at = base;
        for slot in &self.spans {
            if slot.last < at {
                continue;
            }
            if slot.start > last {
                break;
            }
            if slot.start > at {
                spans.push((at, (slot.start - 1).min(last)));
            }
            match slot.last.checked_add(1) {
                Some(next) => at = at.max(next),
                None => return Ok(spans),
            }
        }
        if at <= last {
            spans.push((at, last));
        }
        Ok(spans)
    }

    pub(super) fn insert(&mut self, start: usize, last: usize) -> Result<usize, AddressError> {
        let at = self.spans.partition_point(|slot| slot.start < start);
        if (at > 0 && self.spans[at - 1].last >= start)
            || (at < self.spans.len() && self.spans[at].start <= last)
        {
            return Err(AddressError::NoRegion);
        }
        self.spans
            .try_reserve(1)
            .map_err(|_| AddressError::OutOfMemory)?;
        let id = next_id()?;
        self.spans.insert(
            at,
            Span {
                start,
                last,
                ticket: 0,
                id,
            },
        );
        Ok(id)
    }

    /// 把一段占用对半分成两笔；失败不改动占用。
    pub(crate) fn split(
        &mut self,
        start: usize,
        last: usize,
        reservation_id: usize,
        left_size: usize,
    ) -> Result<(usize, usize, usize), AddressError> {
        let size = last - start + 1;
        if left_size == 0
            || left_size >= size
            || !left_size.is_multiple_of(PAGE_SIZE)
            || !size.is_multiple_of(PAGE_SIZE)
        {
            return Err(AddressError::NotAligned);
        }
        let at = self
            .spans
            .iter()
            .position(|slot| {
                slot.start == start
                    && slot.last == last
                    && slot.ticket == 0
                    && slot.id == reservation_id
            })
            .ok_or(AddressError::NoRegion)?;
        self.spans
            .try_reserve(1)
            .map_err(|_| AddressError::OutOfMemory)?;
        let right = start + left_size;
        let left_id = next_id()?;
        let right_id = next_id()?;
        self.spans[at] = Span {
            start,
            last: right - 1,
            ticket: 0,
            id: left_id,
        };
        self.spans.insert(
            at + 1,
            Span {
                start: right,
                last,
                ticket: 0,
                id: right_id,
            },
        );
        Ok((right, left_id, right_id))
    }

    /// 交还一段占用：generation 与端点都要对上，旧凭证不会释放新占用。
    pub(crate) fn release(&mut self, start: usize, last: usize, reservation_id: usize) -> bool {
        match self.spans.iter().position(|slot| {
            slot.start == start
                && slot.last == last
                && slot.ticket == 0
                && slot.id == reservation_id
        }) {
            Some(at) => {
                self.spans.remove(at);
                true
            }
            None => false,
        }
    }

    /// 该 generation 是否还在占用表里。
    pub(crate) fn has_id(&self, id: usize) -> bool {
        self.spans.iter().any(|slot| slot.id == id)
    }

    /// `[start, last]` 内该 generation 的切片（含票），按地址升序。
    ///
    /// 要求它们恰好拼满这一段；中间夹着别家的占用、或本代已经交还的部分，都是
    /// “这段不由本关系承担”。票为 `0` 表示尚未退休。含尾表示，顶页也不溢出。
    pub(crate) fn pieces(
        &self,
        start: usize,
        last: usize,
        reservation_id: usize,
    ) -> Result<Vec<(usize, usize, usize)>, AddressError> {
        let mut pieces = Vec::new();
        pieces
            .try_reserve(self.spans.len())
            .map_err(|_| AddressError::OutOfMemory)?;
        let mut at = start;
        for slot in &self.spans {
            if slot.last < at {
                continue;
            }
            if slot.start > last {
                break;
            }
            if slot.id != reservation_id || slot.start > at {
                return Err(AddressError::NoRegion);
            }
            let hi = slot.last.min(last);
            pieces.push((at, hi, slot.ticket));
            if hi == last {
                return Ok(pieces);
            }
            at = hi + 1;
        }
        Err(AddressError::NoRegion)
    }

    /// 该 generation 名下仍在退休中的跨度（含票），按地址升序。
    pub(crate) fn retired_of(
        &self,
        reservation_id: usize,
    ) -> Result<Vec<(usize, usize, usize)>, AddressError> {
        let mut retired = Vec::new();
        retired
            .try_reserve(self.spans.len())
            .map_err(|_| AddressError::OutOfMemory)?;
        retired.extend(
            self.spans
                .iter()
                .filter(|slot| slot.id == reservation_id && slot.ticket != 0)
                .map(|slot| (slot.start, slot.last, slot.ticket)),
        );
        Ok(retired)
    }

    pub(crate) fn reserve_cuts(&mut self, count: usize) -> Result<(), AddressError> {
        self.spans
            .try_reserve(count)
            .map_err(|_| AddressError::OutOfMemory)
    }

    /// 把 `[start, last]` 内的占用切成带退休票的段：占位仍留着，但在票结算前不可复用。
    pub(crate) fn retire(&mut self, start: usize, last: usize, reservation_id: usize) -> usize {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(ticket, 0, "address retirement exhausted");
        let mut i = 0;
        while i < self.spans.len() {
            let slot = self.spans[i];
            if slot.ticket != 0
                || slot.id != reservation_id
                || start > slot.last
                || last < slot.start
            {
                i += 1;
                continue;
            }
            self.spans.remove(i);
            if slot.start < start {
                self.spans.insert(
                    i,
                    Span {
                        start: slot.start,
                        last: start - 1,
                        ticket: 0,
                        id: slot.id,
                    },
                );
                i += 1;
            }
            let lo = slot.start.max(start);
            let hi = slot.last.min(last);
            self.spans.insert(
                i,
                Span {
                    start: lo,
                    last: hi,
                    ticket,
                    id: slot.id,
                },
            );
            i += 1;
            if slot.last > last {
                self.spans.insert(
                    i,
                    Span {
                        start: last + 1,
                        last: slot.last,
                        ticket: 0,
                        id: slot.id,
                    },
                );
                i += 1;
            }
        }
        ticket
    }

    /// 结算一票退休：同步完成后移走这张票占用的地址。
    pub(crate) fn reclaim(&mut self, ticket: usize) {
        self.spans.retain(|slot| slot.ticket != ticket);
    }
}

fn next_id() -> Result<usize, AddressError> {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        id.checked_add(1)
            .filter(|next| *next <= isize::MAX as usize)
    })
    .map_err(|_| AddressError::OutOfMemory)
}
