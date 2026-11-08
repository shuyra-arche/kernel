//! 地址选择与占用：窗口只是选址范围，占用是全局单一事实。

mod placement;
pub(crate) use placement::{Placement, PlacementAt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoneError {
    OutOfMemory,
    NotAligned,
    NoRegion,
}

use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use kernel_api::address::PAGE_SIZE;

/// 地址区域：`Normal` / `Kernel` / `Resource` 各有一个动态选址窗口；
/// `Static` 只是地址记账政策：它没有动态窗口，跨度只按 `Exact` 登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionId {
    Normal,
    Kernel,
    Resource,
    Static,
}

/// 一段占用；`last` 是最后一个字节（含），generation 是 `id`。
#[derive(Clone, Copy)]
struct Slot {
    start: usize,
    last: usize,
    ticket: usize,
    id: usize,
    region: RegionId,
}

/// 一个动态选址窗口；`last` 是最后一个可用地址（含）。
#[derive(Clone, Copy)]
struct Window {
    id: RegionId,
    base: usize,
    last: usize,
}

/// 全局 VA 占用。
///
/// 窗口只决定 `Lowest` / `Highest` 的选址范围；占用本身是一份全局有序表，
/// 固定的 `Exact` 跨度与动态选择走同一份重叠检查。
/// 端点按最后一个字节（含）表示，因此 `usize::MAX` 结尾的固定跨度也合法。
pub(crate) struct Zone<C> {
    windows: Vec<Window>,
    slots: Vec<Slot>,
    cleanups: Vec<(usize, C)>,
}

impl<C> Zone<C> {
    pub(crate) const fn new() -> Self {
        Self {
            windows: Vec::new(),
            slots: Vec::new(),
            cleanups: Vec::new(),
        }
    }

    /// 登记动态窗口 `[base, last]`（含端点）。
    ///
    /// `Static` 只是地址记账政策，没有动态窗口：给这类区域建窗就是 NoRegion。
    pub(crate) fn attach(
        &mut self,
        id: RegionId,
        base: usize,
        last: usize,
    ) -> Result<(), ZoneError> {
        if base > last
            || id == RegionId::Static
            || self.windows.iter().any(|window| window.id == id)
        {
            return Err(ZoneError::NoRegion);
        }
        self.windows
            .try_reserve(1)
            .map_err(|_| ZoneError::OutOfMemory)?;
        self.windows.push(Window { id, base, last });
        Ok(())
    }

    /// 抬高已有窗口的选址下界；已登记的占用留在表里。
    pub(crate) fn advance(&mut self, id: RegionId, base: usize) -> Result<(), ZoneError> {
        let window = self
            .windows
            .iter_mut()
            .find(|window| window.id == id)
            .ok_or(ZoneError::NoRegion)?;
        if base < window.base || base > window.last {
            return Err(ZoneError::NoRegion);
        }
        window.base = base;
        Ok(())
    }

    /// 按 `placement` 选段并登记占用：不分配数据、不发布 PTE。
    /// 返回选中的起点与占用身份（generation）。
    pub(crate) fn reserve(&mut self, placement: Placement) -> Result<(usize, usize), ZoneError> {
        let size = placement.size();
        let region = placement.region();
        let start = match placement.at() {
            PlacementAt::Exact(start) => start.as_usize(),
            PlacementAt::Lowest => {
                let window = self.window(region)?;
                let align = placement.align();
                self.free_spans(window)?
                    .into_iter()
                    .find_map(|(lo, hi)| {
                        let at = lo.checked_next_multiple_of(align)?;
                        hi.checked_sub(at)
                            .is_some_and(|gap| gap >= size - 1)
                            .then_some(at)
                    })
                    .ok_or(ZoneError::OutOfMemory)?
            }
            PlacementAt::Highest => {
                let window = self.window(region)?;
                self.free_spans(window)?
                    .into_iter()
                    .rev()
                    .find_map(|(lo, hi)| {
                        let at = hi.checked_sub(size - 1)?;
                        (at >= lo).then_some(at)
                    })
                    .ok_or(ZoneError::OutOfMemory)?
            }
        };
        let last = start.checked_add(size - 1).ok_or(ZoneError::NoRegion)?;
        let id = self.insert(region, start, last)?;
        Ok((start, id))
    }

    fn window(&self, id: RegionId) -> Result<Window, ZoneError> {
        self.windows
            .iter()
            .copied()
            .find(|window| window.id == id)
            .ok_or(ZoneError::NoRegion)
    }

    /// 窗口 `[base, last]` 是否含住 `[start, last]`。
    pub(crate) fn contains(&self, id: RegionId, start: usize, last: usize) -> bool {
        self.window(id)
            .is_ok_and(|window| start >= window.base && last <= window.last)
    }

    /// 窗口内未被占用的区间（含端点），按地址升序。
    fn free_spans(&self, window: Window) -> Result<Vec<(usize, usize)>, ZoneError> {
        let mut spans = Vec::new();
        spans
            .try_reserve(self.slots.len().saturating_add(1))
            .map_err(|_| ZoneError::OutOfMemory)?;
        let mut at = window.base;
        for slot in &self.slots {
            if slot.last < at {
                continue;
            }
            if slot.start > window.last {
                break;
            }
            if slot.start > at {
                spans.push((at, (slot.start - 1).min(window.last)));
            }
            match slot.last.checked_add(1) {
                Some(next) => at = at.max(next),
                None => return Ok(spans),
            }
        }
        if at <= window.last {
            spans.push((at, window.last));
        }
        Ok(spans)
    }

    fn insert(&mut self, region: RegionId, start: usize, last: usize) -> Result<usize, ZoneError> {
        let at = self.slots.partition_point(|slot| slot.start < start);
        if (at > 0 && self.slots[at - 1].last >= start)
            || (at < self.slots.len() && self.slots[at].start <= last)
        {
            return Err(ZoneError::NoRegion);
        }
        self.slots
            .try_reserve(1)
            .map_err(|_| ZoneError::OutOfMemory)?;
        let id = next_id()?;
        self.slots.insert(
            at,
            Slot {
                start,
                last,
                ticket: 0,
                id,
                region,
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
    ) -> Result<(usize, usize, usize), ZoneError> {
        let size = last - start + 1;
        if left_size == 0
            || left_size >= size
            || !left_size.is_multiple_of(PAGE_SIZE)
            || !size.is_multiple_of(PAGE_SIZE)
        {
            return Err(ZoneError::NotAligned);
        }
        let at = self
            .slots
            .iter()
            .position(|slot| {
                slot.start == start
                    && slot.last == last
                    && slot.ticket == 0
                    && slot.id == reservation_id
            })
            .ok_or(ZoneError::NoRegion)?;
        self.slots
            .try_reserve(1)
            .map_err(|_| ZoneError::OutOfMemory)?;
        let right = start + left_size;
        let left_id = next_id()?;
        let right_id = next_id()?;
        let region = self.slots[at].region;
        self.slots[at] = Slot {
            start,
            last: right - 1,
            ticket: 0,
            id: left_id,
            region,
        };
        self.slots.insert(
            at + 1,
            Slot {
                start: right,
                last,
                ticket: 0,
                id: right_id,
                region,
            },
        );
        Ok((right, left_id, right_id))
    }

    /// 交还一段占用：generation 与端点都要对上，旧凭证不会释放新占用。
    pub(crate) fn release(&mut self, start: usize, last: usize, reservation_id: usize) -> bool {
        match self.slots.iter().position(|slot| {
            slot.start == start
                && slot.last == last
                && slot.ticket == 0
                && slot.id == reservation_id
        }) {
            Some(at) => {
                self.slots.remove(at);
                self.cleanups.retain(|(id, _)| *id != reservation_id);
                true
            }
            None => false,
        }
    }

    /// 发布前先预留该 generation 的清理载荷：失败时不进入发布。
    pub(crate) fn prepare_cleanup(
        &mut self,
        reservation_id: usize,
        cleanup: impl FnOnce() -> Result<C, ZoneError>,
    ) -> Result<(), ZoneError> {
        if self.cleanups.iter().any(|(id, _)| *id == reservation_id) {
            return Ok(());
        }
        self.cleanups
            .try_reserve(1)
            .map_err(|_| ZoneError::OutOfMemory)?;
        let node = cleanup()?;
        self.cleanups.push((reservation_id, node));
        Ok(())
    }

    pub(crate) fn take_cleanup(&mut self, reservation_id: usize) -> Option<C> {
        self.cleanups
            .iter()
            .position(|(id, _)| *id == reservation_id)
            .map(|at| self.cleanups.swap_remove(at).1)
    }

    /// 该 generation 是否还在占用表里。
    pub(crate) fn has_id(&self, id: usize) -> bool {
        self.slots.iter().any(|slot| slot.id == id)
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
    ) -> Result<Vec<(usize, usize, usize)>, ZoneError> {
        let mut pieces = Vec::new();
        pieces
            .try_reserve(self.slots.len())
            .map_err(|_| ZoneError::OutOfMemory)?;
        let mut at = start;
        for slot in &self.slots {
            if slot.last < at {
                continue;
            }
            if slot.start > last {
                break;
            }
            if slot.id != reservation_id || slot.start > at {
                return Err(ZoneError::NoRegion);
            }
            let hi = slot.last.min(last);
            pieces.push((at, hi, slot.ticket));
            if hi == last {
                return Ok(pieces);
            }
            at = hi + 1;
        }
        Err(ZoneError::NoRegion)
    }

    /// 该 generation 名下仍在退休中的跨度（含票），按地址升序。
    pub(crate) fn retired_of(
        &self,
        reservation_id: usize,
    ) -> Result<Vec<(usize, usize, usize)>, ZoneError> {
        let mut retired = Vec::new();
        retired
            .try_reserve(self.slots.len())
            .map_err(|_| ZoneError::OutOfMemory)?;
        retired.extend(
            self.slots
                .iter()
                .filter(|slot| slot.id == reservation_id && slot.ticket != 0)
                .map(|slot| (slot.start, slot.last, slot.ticket)),
        );
        Ok(retired)
    }

    pub(crate) fn reserve_cuts(&mut self, count: usize) -> Result<(), ZoneError> {
        self.slots
            .try_reserve(count)
            .map_err(|_| ZoneError::OutOfMemory)
    }

    /// 把 `[start, last]` 内的占用切成带退休票的段：占位仍留着，但在票结算前不可复用。
    pub(crate) fn retire(&mut self, start: usize, last: usize, reservation_id: usize) -> usize {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(ticket, 0, "address retirement exhausted");
        let mut i = 0;
        while i < self.slots.len() {
            let slot = self.slots[i];
            if slot.ticket != 0
                || slot.id != reservation_id
                || start > slot.last
                || last < slot.start
            {
                i += 1;
                continue;
            }
            self.slots.remove(i);
            if slot.start < start {
                self.slots.insert(
                    i,
                    Slot {
                        start: slot.start,
                        last: start - 1,
                        ticket: 0,
                        id: slot.id,
                        region: slot.region,
                    },
                );
                i += 1;
            }
            let lo = slot.start.max(start);
            let hi = slot.last.min(last);
            self.slots.insert(
                i,
                Slot {
                    start: lo,
                    last: hi,
                    ticket,
                    id: slot.id,
                    region: slot.region,
                },
            );
            i += 1;
            if slot.last > last {
                self.slots.insert(
                    i,
                    Slot {
                        start: last + 1,
                        last: slot.last,
                        ticket: 0,
                        id: slot.id,
                        region: slot.region,
                    },
                );
                i += 1;
            }
        }
        ticket
    }

    /// 结算一票退休：移走带票的占位；该 generation 的最后一段消失时一并解除清理预留。
    pub(crate) fn reclaim(&mut self, ticket: usize) {
        let removed = self
            .slots
            .iter()
            .find(|slot| slot.ticket == ticket)
            .map(|slot| slot.id);
        self.slots.retain(|slot| slot.ticket != ticket);
        if let Some(reservation_id) = removed
            && !self.slots.iter().any(|slot| slot.id == reservation_id)
        {
            self.cleanups.retain(|(id, _)| *id != reservation_id);
        }
    }
}

fn next_id() -> Result<usize, ZoneError> {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
        id.checked_add(1)
            .filter(|next| *next <= isize::MAX as usize)
    })
    .map_err(|_| ZoneError::OutOfMemory)
}
