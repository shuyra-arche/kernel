use alloc::boxed::Box;
use alloc::sync::Arc;

#[cfg(debug_assertions)]
use crate::metal::generic::mmu::{Attributes, PageTable};
#[cfg(debug_assertions)]
use crate::metal::mmu::{Geometry, Table};
#[cfg(debug_assertions)]
use kernel_api::address::PAGE_SIZE;

#[cfg(debug_assertions)]
use super::Nest;
use super::Span;

mod node;
pub(crate) mod source;

pub(super) use node::IndexNode;

/// 关系索引：AVL 根与关系计数；键是关系最小区间起点，节点缓存子树高度与最大
/// 区间终点。
pub(super) struct Index {
    root: Option<Box<IndexNode>>,
    len: usize,
}

impl Index {
    pub(super) const fn new() -> Self {
        Self { root: None, len: 0 }
    }

    /// 存量关系数。
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// 接入一个已建好的节点。
    pub(super) fn insert(&mut self, node: Box<IndexNode>) {
        self.root = Some(insert(self.root.take(), node));
        self.len += 1;
    }

    /// 按关系身份摘除节点；摘到才减计数。
    pub(super) fn remove(&mut self, reservation: &Arc<Span>) -> bool {
        let (root, removed) = remove_reservation(self.root.take(), reservation);
        self.root = root;
        if removed {
            self.len -= 1;
        }
        removed
    }

    /// 覆盖 `address` 的关系。
    pub(super) fn find(&self, address: usize) -> Option<Arc<Span>> {
        find_reservation(self.root.as_deref(), address)
    }

    /// 与 [lo, hi] 相交的第一个关系。
    pub(super) fn first_overlap(&self, lo: usize, hi: usize) -> Option<Arc<Span>> {
        first_overlap(self.root.as_deref(), lo, hi)
    }

    /// 关系区间变化后重算其节点缓存。
    pub(super) fn refresh(&mut self, reservation: &Arc<Span>) -> bool {
        refresh(&mut self.root, reservation)
    }

    /// 索引自身的不变量：节点数等于计数，键、高度与缓存都要对。
    #[cfg(debug_assertions)]
    pub(super) fn audit(&self) {
        let (_, _, count) = audit_index(self.root.as_deref(), None, None);
        assert_eq!(count, self.len, "nest index length");
    }

    /// 索引内的关系与地址空间、硬件页表一致。
    #[cfg(debug_assertions)]
    pub(super) fn audit_reservations(
        &self,
        nest: &Nest,
        root: &Table,
        geo: Geometry,
        memos: &super::memo::Memos,
    ) {
        audit_reservations(self.root.as_deref(), nest, root, geo, memos);
    }
}

/// 按最小区间起点接入节点。
fn insert(root: Option<Box<IndexNode>>, node: Box<IndexNode>) -> Box<IndexNode> {
    let Some(mut root) = root else { return node };
    if node.start < root.start {
        root.left = Some(insert(root.left.take(), node));
    } else {
        root.right = Some(insert(root.right.take(), node));
    }
    IndexNode::balance(root)
}

/// 按关系身份摘除节点，返回 (新根, 是否摘到)。
fn remove_reservation(
    root: Option<Box<IndexNode>>,
    reservation: &Arc<Span>,
) -> (Option<Box<IndexNode>>, bool) {
    let Some(mut node) = root else {
        return (None, false);
    };
    if Arc::ptr_eq(&node.reservation, reservation) {
        return (IndexNode::detach(node), true);
    }
    let (left, found) = remove_reservation(node.left.take(), reservation);
    node.left = left;
    if found {
        return (Some(IndexNode::balance(node)), true);
    }
    let (right, found) = remove_reservation(node.right.take(), reservation);
    node.right = right;
    if found {
        (Some(IndexNode::balance(node)), true)
    } else {
        (Some(node), false)
    }
}

/// 覆盖 `address` 的关系（先左子树，再看本节点，再右子树）。
fn find_reservation(root: Option<&IndexNode>, address: usize) -> Option<Arc<Span>> {
    let node = root?;
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= address)
    {
        if let Some(reservation) = find_reservation(node.left.as_deref(), address) {
            return Some(reservation);
        }
    }
    if node.reservation.overlaps(address, address) {
        return Some(node.reservation.clone());
    }
    (node.start.as_usize() <= address)
        .then(|| find_reservation(node.right.as_deref(), address))
        .flatten()
}

/// 与 [lo, hi] 相交的第一个关系。
fn first_overlap(root: Option<&IndexNode>, lo: usize, hi: usize) -> Option<Arc<Span>> {
    let node = root?;
    if node.max_end < lo {
        return None;
    }
    if node.left.as_ref().is_some_and(|left| left.max_end >= lo) {
        if let Some(reservation) = first_overlap(node.left.as_deref(), lo, hi) {
            return Some(reservation);
        }
    }
    if node.reservation.overlaps(lo, hi) {
        return Some(node.reservation.clone());
    }
    if node.start.as_usize() <= hi {
        first_overlap(node.right.as_deref(), lo, hi)
    } else {
        None
    }
}

fn refresh(root: &mut Option<Box<IndexNode>>, reservation: &Arc<Span>) -> bool {
    let Some(node) = root.as_deref_mut() else {
        return false;
    };
    if Arc::ptr_eq(&node.reservation, reservation) {
        node.update();
        return true;
    }
    if refresh(&mut node.left, reservation) || refresh(&mut node.right, reservation) {
        node.update();
        true
    } else {
        false
    }
}

#[cfg(debug_assertions)]
fn audit_index(
    node: Option<&IndexNode>,
    lower: Option<usize>,
    upper: Option<usize>,
) -> (usize, usize, usize) {
    let Some(node) = node else { return (0, 0, 0) };
    let key = node.start.as_usize();
    assert!(lower.is_none_or(|lo| lo < key), "AVL key below lower bound");
    assert!(upper.is_none_or(|hi| key < hi), "AVL key above upper bound");
    let (left_height, left_end, left_count) = audit_index(node.left.as_deref(), lower, Some(key));
    let (right_height, right_end, right_count) =
        audit_index(node.right.as_deref(), Some(key), upper);
    let local = node.reservation.active_snapshot();
    assert!(
        !local.is_empty(),
        "indexed reservation has no active ranges"
    );
    assert!(
        local.windows(2).all(|pair| pair[0].1 < pair[1].0),
        "reservation ranges overlap or are unordered"
    );
    for (lo, hi) in &local {
        assert!(
            !overlaps_other(node, &node.reservation, *lo, *hi),
            "distinct mappings overlap in the nest index"
        );
    }
    assert_eq!(
        key,
        local.iter().map(|(start, _)| *start).min().unwrap(),
        "AVL key differs from active Span range"
    );
    let local_end = local.iter().map(|(_, end)| *end).max().unwrap();
    let expected_height = 1 + left_height.max(right_height);
    assert_eq!(node.height as usize, expected_height, "AVL height cache");
    assert!(left_height.abs_diff(right_height) <= 1, "AVL balance");
    let expected_end = local_end.max(left_end).max(right_end);
    assert_eq!(node.max_end, expected_end, "AVL max_end cache");
    (expected_height, expected_end, left_count + right_count + 1)
}

#[cfg(debug_assertions)]
fn overlaps_other(node: &IndexNode, reservation: &Arc<Span>, lo: usize, hi: usize) -> bool {
    if node.max_end < lo {
        return false;
    }
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= lo && overlaps_other(left, reservation, lo, hi))
    {
        return true;
    }
    if !Arc::ptr_eq(&node.reservation, reservation) && node.reservation.overlaps(lo, hi) {
        return true;
    }
    node.start.as_usize() <= hi
        && node
            .right
            .as_ref()
            .is_some_and(|right| overlaps_other(right, reservation, lo, hi))
}

#[cfg(debug_assertions)]
fn audit_reservations(
    node: Option<&IndexNode>,
    nest: &Nest,
    root: &Table,
    geo: Geometry,
    memos: &super::memo::Memos,
) {
    let Some(node) = node else { return };
    audit_reservations(node.left.as_deref(), nest, root, geo, memos);
    let reservation = &node.reservation;
    assert!(
        reservation
            .reservation
            .nest()
            .is_some_and(|owner| core::ptr::eq(
                Arc::as_ptr(&owner) as *const Nest,
                nest as *const Nest
            )),
        "indexed Span belongs to another Nest"
    );
    let state = reservation.state.lock();
    if let Some(source) = &state.source {
        assert!(
            !source.is_borrowed() || memos.published(reservation).next().is_none(),
            "borrowed Span also holds published Cells"
        );
        assert_eq!(
            source.page_count().checked_bytes(),
            Some(reservation.initial_size()),
            "Span source extent differs from its initial range"
        );
        assert_eq!(
            source.level(),
            reservation.level(),
            "implicit source geometry"
        );
    }
    assert!(
        reservation
            .initial_size()
            .is_multiple_of(reservation.grain() * PAGE_SIZE)
    );
    for (page, cell) in memos.published(reservation) {
        let address = reservation.initial_start() + page * PAGE_SIZE;
        assert!(
            state.covers(address.as_usize(), address.as_usize()),
            "published Page lies outside live Span ranges"
        );
        let (physical, flags) = root
            .walk_ref_geometry(address, geo)
            .expect("published Page has no hardware PTE");
        assert_eq!(
            physical.as_usize(),
            cell.physical_address(),
            "published Page PTE physical address"
        );
        let expected = reservation.leaf_attributes(
            state
                .range_at(address.as_usize())
                .expect("Page range exists")
                .current,
        );
        let mask = Attributes::READ
            | Attributes::WRITE
            | Attributes::EXECUTE
            | Attributes::USER
            | Attributes::GLOBAL;
        assert_eq!(
            (flags & mask).bits(),
            (expected & mask).bits(),
            "published Page PTE modifiability differ from Span state"
        );
    }
    drop(state);
    audit_reservations(node.right.as_deref(), nest, root, geo, memos);
}
