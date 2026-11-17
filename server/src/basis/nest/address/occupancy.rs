//! 同一个 Nest 的全部地址占用，按起点排序。固定与动态选址共用这份互斥记录。
//! 含尾区间允许占用延伸到 usize::MAX；退役中的区间继续占位，直到票结算。

use alloc::vec::Vec;

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

/// Prepared while the owning Layout lock remains held through commit. This is
/// unpublished capacity and identity span, not a second occupancy ledger.
pub(crate) struct PreparedRetirement {
    pieces: Vec<(usize, usize, usize)>,
    identities: Option<core::ops::RangeInclusive<usize>>,
    span: usize,
}

impl PreparedRetirement {
    pub(crate) fn len(&self) -> usize {
        self.pieces.len()
    }
}

impl Occupancy {
    pub(super) const fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Scan the sole occupancy ledger without allocating under its lock.
    /// Inclusive endpoints also represent the final page ending at usize::MAX.
    pub(super) fn free_spans(
        &self,
        base: usize,
        last: usize,
    ) -> impl DoubleEndedIterator<Item = (usize, usize)> + '_ {
        (0..=self.spans.len()).filter_map(move |i| {
            let lo = if i == 0 {
                base
            } else {
                self.spans[i - 1].last.checked_add(1)?.max(base)
            };
            let hi = if i == self.spans.len() {
                last
            } else {
                self.spans[i].start.checked_sub(1)?.min(last)
            };
            (lo <= hi).then_some((lo, hi))
        })
    }

    pub(super) fn insert(&mut self, start: usize, last: usize) -> Result<usize, AddressError> {
        let at = self.spans.partition_point(|slot| slot.start < start);
        if (at > 0 && self.spans[at - 1].last >= start)
            || (at < self.spans.len() && self.spans[at].start <= last)
        {
            return Err(AddressError::Occupied);
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
        generation: usize,
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
                    && slot.id == generation
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
    pub(crate) fn release(&mut self, start: usize, last: usize, generation: usize) -> bool {
        match self.spans.iter().position(|slot| {
            slot.start == start && slot.last == last && slot.ticket == 0 && slot.id == generation
        }) {
            Some(at) => {
                self.spans.remove(at);
                true
            }
            None => false,
        }
    }

    /// Release a page-aligned subrange of one exact occupancy generation.
    /// The remaining left/right pieces keep the same generation identity, so a
    /// stale credential cannot release a newer occupant.
    pub(crate) fn release_range(&mut self, start: usize, last: usize, generation: usize) -> bool {
        if start > last
            || !start.is_multiple_of(PAGE_SIZE)
            || last % PAGE_SIZE != PAGE_SIZE - 1
        {
            return false;
        }
        let Some(at) = self.spans.iter().position(|slot| {
            slot.start <= start && slot.last >= last && slot.id == generation && slot.ticket == 0
        }) else {
            return false;
        };
        let whole_start = self.spans[at].start;
        let whole_last = self.spans[at].last;
        if start == whole_start && last == whole_last {
            self.spans.remove(at);
            return true;
        }
        if start == whole_start {
            let Some(next) = last.checked_add(1) else {
                return false;
            };
            self.spans[at].start = next;
            return true;
        }
        if last == whole_last {
            self.spans[at].last = start - 1;
            return true;
        }
        if self.spans.try_reserve(1).is_err() {
            return false;
        }
        let Some(next) = last.checked_add(1) else {
            return false;
        };
        let right = Span {
            start: next,
            last: whole_last,
            ticket: 0,
            id: generation,
        };
        self.spans[at].last = start - 1;
        self.spans.insert(at + 1, right);
        true
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
        generation: usize,
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
            if slot.id != generation || slot.start > at {
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
        generation: usize,
    ) -> Result<Vec<(usize, usize, usize)>, AddressError> {
        let mut retired = Vec::new();
        retired
            .try_reserve(self.spans.len())
            .map_err(|_| AddressError::OutOfMemory)?;
        retired.extend(
            self.spans
                .iter()
                .filter(|slot| slot.id == generation && slot.ticket != 0)
                .map(|slot| (slot.start, slot.last, slot.ticket)),
        );
        Ok(retired)
    }

    /// Reserve every cut and ticket identity before changing occupancy or PTEs.
    pub(crate) fn prepare_retirement(
        &mut self,
        start: usize,
        last: usize,
        span: usize,
    ) -> Result<PreparedRetirement, AddressError> {
        let pieces = self.pieces(start, last, span)?;
        let fresh = pieces.iter().filter(|(_, _, ticket)| *ticket == 0).count();
        self.spans
            .try_reserve(fresh.checked_mul(2).ok_or(AddressError::OutOfMemory)?)
            .map_err(|_| AddressError::OutOfMemory)?;
        static NEXT: crate::basis::name::Serial =
            crate::basis::name::Serial::new(1, isize::MAX as usize);
        let identities = if fresh == 0 {
            None
        } else {
            Some(NEXT.reserve(fresh).ok_or(AddressError::OutOfMemory)?)
        };
        Ok(PreparedRetirement {
            pieces,
            identities,
            span,
        })
    }

    /// Commit under the same Layout lock, after all other owners are prepared.
    /// No allocation or fallible ticket preparation remains here.
    pub(crate) fn retire(
        &mut self,
        mut prepared: PreparedRetirement,
    ) -> Vec<(usize, usize, usize)> {
        for (start, last, ticket) in &mut prepared.pieces {
            if *ticket == 0 {
                *ticket = prepared
                    .identities
                    .as_mut()
                    .and_then(Iterator::next)
                    .expect("prepared retirement identity");
                self.retire_piece(*start, *last, prepared.span, *ticket);
            }
        }
        prepared.pieces
    }

    /// Occupancy remains reserved until the exact ticket settles.
    fn retire_piece(&mut self, start: usize, last: usize, generation: usize, ticket: usize) {
        let mut i = 0;
        while i < self.spans.len() {
            let slot = self.spans[i];
            if slot.ticket != 0 || slot.id != generation || start > slot.last || last < slot.start {
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
    }

    /// Caller proved every hardware obligation of this span settled.
    pub(crate) fn release_settled(&mut self, id: usize) {
        self.spans.retain(|span| span.id != id);
    }

    /// 结算一票退休：同步完成后移走这张票占用的地址。
    pub(crate) fn reclaim(&mut self, ticket: usize) {
        self.spans.retain(|slot| slot.ticket != ticket);
    }
}

fn next_id() -> Result<usize, AddressError> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate().ok_or(AddressError::OutOfMemory)
}
