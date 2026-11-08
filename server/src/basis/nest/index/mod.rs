use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(debug_assertions)]
use crate::metal::generic::mmu::{Attributes, PageTable};
#[cfg(debug_assertions)]
use crate::metal::mmu::{Geometry, Table};
#[cfg(debug_assertions)]
use kernel_api::address::PAGE_SIZE;

use super::Mapping;
#[cfg(debug_assertions)]
use super::Nest;

mod node;

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
    pub(super) fn remove(&mut self, mapping: &Arc<Mapping>) -> bool {
        let (root, removed) = remove_mapping(self.root.take(), mapping);
        self.root = root;
        if removed {
            self.len -= 1;
        }
        removed
    }

    /// 覆盖 `address` 的关系。
    pub(super) fn find(&self, address: usize) -> Option<Arc<Mapping>> {
        find_mapping(self.root.as_deref(), address)
    }

    /// 与 [lo, hi] 相交的第一个关系。
    pub(super) fn first_overlap(&self, lo: usize, hi: usize) -> Option<Arc<Mapping>> {
        first_overlap(self.root.as_deref(), lo, hi)
    }

    /// 中序收集全部关系。
    pub(super) fn collect(&self, mappings: &mut Vec<Arc<Mapping>>) {
        collect_mappings(self.root.as_deref(), mappings)
    }

    /// 关系区间变化后重算其节点缓存。
    pub(super) fn refresh(&mut self, mapping: &Arc<Mapping>) -> bool {
        refresh(&mut self.root, mapping)
    }

    /// 撕销全部索引。
    pub(super) fn clear(&mut self) {
        self.root = None;
        self.len = 0;
    }

    /// 索引自身的不变量：节点数等于计数，键、高度与缓存都要对。
    #[cfg(debug_assertions)]
    pub(super) fn audit(&self) {
        let (_, _, count) = audit_index(self.root.as_deref(), None, None);
        assert_eq!(count, self.len, "nest index length");
    }

    /// 索引内的关系与地址空间、硬件页表一致。
    #[cfg(debug_assertions)]
    pub(super) fn audit_mappings(&self, nest: &Nest, root: &Table, geo: Geometry) {
        audit_mappings(self.root.as_deref(), nest, root, geo);
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
fn remove_mapping(
    root: Option<Box<IndexNode>>,
    mapping: &Arc<Mapping>,
) -> (Option<Box<IndexNode>>, bool) {
    let Some(mut node) = root else {
        return (None, false);
    };
    if Arc::ptr_eq(&node.mapping, mapping) {
        return (IndexNode::detach(node), true);
    }
    let (left, found) = remove_mapping(node.left.take(), mapping);
    node.left = left;
    if found {
        return (Some(IndexNode::balance(node)), true);
    }
    let (right, found) = remove_mapping(node.right.take(), mapping);
    node.right = right;
    if found {
        (Some(IndexNode::balance(node)), true)
    } else {
        (Some(node), false)
    }
}

/// 覆盖 `address` 的关系（先左子树，再看本节点，再右子树）。
fn find_mapping(root: Option<&IndexNode>, address: usize) -> Option<Arc<Mapping>> {
    let node = root?;
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= address)
    {
        if let Some(mapping) = find_mapping(node.left.as_deref(), address) {
            return Some(mapping);
        }
    }
    if node.mapping.overlaps(address, address) {
        return Some(node.mapping.clone());
    }
    (node.start.as_usize() <= address)
        .then(|| find_mapping(node.right.as_deref(), address))
        .flatten()
}

/// 与 [lo, hi] 相交的第一个关系。
fn first_overlap(root: Option<&IndexNode>, lo: usize, hi: usize) -> Option<Arc<Mapping>> {
    let node = root?;
    if node.max_end < lo {
        return None;
    }
    if node.left.as_ref().is_some_and(|left| left.max_end >= lo) {
        if let Some(mapping) = first_overlap(node.left.as_deref(), lo, hi) {
            return Some(mapping);
        }
    }
    if node.mapping.overlaps(lo, hi) {
        return Some(node.mapping.clone());
    }
    if node.start.as_usize() <= hi {
        first_overlap(node.right.as_deref(), lo, hi)
    } else {
        None
    }
}

fn collect_mappings(node: Option<&IndexNode>, mappings: &mut Vec<Arc<Mapping>>) {
    let Some(node) = node else { return };
    collect_mappings(node.left.as_deref(), mappings);
    mappings.push(node.mapping.clone());
    collect_mappings(node.right.as_deref(), mappings);
}

fn refresh(root: &mut Option<Box<IndexNode>>, mapping: &Arc<Mapping>) -> bool {
    let Some(node) = root.as_deref_mut() else {
        return false;
    };
    if Arc::ptr_eq(&node.mapping, mapping) {
        node.update();
        return true;
    }
    if refresh(&mut node.left, mapping) || refresh(&mut node.right, mapping) {
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
    let local = node.mapping.active_snapshot();
    assert!(!local.is_empty(), "indexed mapping has no active ranges");
    assert!(
        local.windows(2).all(|pair| pair[0].1 < pair[1].0),
        "mapping ranges overlap or are unordered"
    );
    for (lo, hi) in &local {
        assert!(
            !overlaps_other(node, &node.mapping, *lo, *hi),
            "distinct mappings overlap in the nest index"
        );
    }
    assert_eq!(
        key,
        local.iter().map(|(start, _)| *start).min().unwrap(),
        "AVL key differs from active Mapping range"
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
fn overlaps_other(node: &IndexNode, mapping: &Arc<Mapping>, lo: usize, hi: usize) -> bool {
    if node.max_end < lo {
        return false;
    }
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= lo && overlaps_other(left, mapping, lo, hi))
    {
        return true;
    }
    if !Arc::ptr_eq(&node.mapping, mapping) && node.mapping.overlaps(lo, hi) {
        return true;
    }
    node.start.as_usize() <= hi
        && node
            .right
            .as_ref()
            .is_some_and(|right| overlaps_other(right, mapping, lo, hi))
}

#[cfg(debug_assertions)]
fn audit_mappings(node: Option<&IndexNode>, nest: &Nest, root: &Table, geo: Geometry) {
    let Some(node) = node else { return };
    audit_mappings(node.left.as_deref(), nest, root, geo);
    let mapping = &node.mapping;
    assert!(
        mapping
            .reservation
            .nest()
            .is_some_and(|owner| core::ptr::eq(
                Arc::as_ptr(&owner) as *const Nest,
                nest as *const Nest
            )),
        "indexed Mapping belongs to another Nest"
    );
    let state = mapping.state.lock();
    assert!(
        !state.source.is_borrowed() || state.published.is_empty(),
        "borrowed Mapping also holds published Cells"
    );
    assert!(
        state.published.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "published Page indexes are not strictly ordered"
    );
    assert_eq!(
        state.source.page_count().checked_bytes(),
        Some(mapping.initial_size()),
        "Mapping source extent differs from its initial range"
    );
    for (page, cell) in &state.published {
        let address = mapping.initial_start() + page * PAGE_SIZE;
        assert!(
            state.covers(address.as_usize(), address.as_usize()),
            "published Page lies outside live Mapping ranges"
        );
        let (physical, flags) = root
            .walk_ref_geometry(address, geo)
            .expect("published Page has no hardware PTE");
        assert_eq!(
            physical.as_usize(),
            cell.physical_address(),
            "published Page PTE physical address"
        );
        let expected = mapping.leaf_attributes(
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
            "published Page PTE modifiability differ from Mapping state"
        );
    }
    drop(state);
    audit_mappings(node.right.as_deref(), nest, root, geo);
}
