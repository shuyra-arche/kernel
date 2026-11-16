use alloc::boxed::Box;
use alloc::sync::Arc;

#[cfg(debug_assertions)]
use crate::metal::generic::mmu::internal::{Attributes, PageTable};
#[cfg(debug_assertions)]
use crate::metal::mmu::internal::{Geometry, Table};
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
    /// Close native recovery admission without running continuations under the
    /// Nest lock. Sign delivery queues the already allocated wake endpoint.
    pub(super) fn close_faults(&self) {
        fn visit(node: Option<&IndexNode>) {
            if let Some(node) = node {
                node.span.faults.lock().close();
                visit(node.left.as_deref());
                visit(node.right.as_deref());
            }
        }
        visit(self.root.as_deref());
    }
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
    pub(super) fn remove(&mut self, span: &Arc<Span>) -> bool {
        let (root, removed) = remove_span(self.root.take(), span);
        self.root = root;
        if removed {
            self.len -= 1;
        }
        removed
    }

    /// 覆盖 `address` 的关系。
    pub(super) fn find(&self, address: usize) -> Option<Arc<Span>> {
        find_span(self.root.as_deref(), address)
    }

    /// Visit every indexed Span until the predicate accepts one.
    pub(super) fn any(&self, mut predicate: impl FnMut(&Arc<Span>) -> bool) -> bool {
        fn visit(node: Option<&IndexNode>, predicate: &mut impl FnMut(&Arc<Span>) -> bool) -> bool {
            let Some(node) = node else { return false };
            visit(node.left.as_deref(), predicate)
                || predicate(&node.span)
                || visit(node.right.as_deref(), predicate)
        }
        visit(self.root.as_deref(), &mut predicate)
    }

    /// 与 [lo, hi] 相交的第一个关系。
    pub(super) fn first_overlap(&self, lo: usize, hi: usize) -> Option<Arc<Span>> {
        first_overlap(self.root.as_deref(), lo, hi)
    }

    /// 关系区间变化后重算其节点缓存。
    pub(super) fn refresh(&mut self, span: &Arc<Span>) -> bool {
        refresh(&mut self.root, span)
    }

    /// 索引自身的不变量：节点数等于计数，键、高度与缓存都要对。
    #[cfg(debug_assertions)]
    pub(super) fn audit(&self) {
        let (_, _, count) = audit_index(self.root.as_deref(), None, None);
        assert_eq!(count, self.len, "nest index length");
    }

    /// 索引内的关系与地址空间、硬件页表一致。
    #[cfg(debug_assertions)]
    pub(super) fn audit_spans(&self, nest: &Nest, root: &Table, geo: Geometry) {
        audit_spans(self.root.as_deref(), nest, root, geo);
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
fn remove_span(root: Option<Box<IndexNode>>, span: &Arc<Span>) -> (Option<Box<IndexNode>>, bool) {
    let Some(mut node) = root else {
        return (None, false);
    };
    if Arc::ptr_eq(&node.span, span) {
        return (IndexNode::detach(node), true);
    }
    let (left, found) = remove_span(node.left.take(), span);
    node.left = left;
    if found {
        return (Some(IndexNode::balance(node)), true);
    }
    let (right, found) = remove_span(node.right.take(), span);
    node.right = right;
    if found {
        (Some(IndexNode::balance(node)), true)
    } else {
        (Some(node), false)
    }
}

/// 覆盖 `address` 的关系（先左子树，再看本节点，再右子树）。
fn find_span(root: Option<&IndexNode>, address: usize) -> Option<Arc<Span>> {
    let node = root?;
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= address)
    {
        if let Some(span) = find_span(node.left.as_deref(), address) {
            return Some(span);
        }
    }
    if node.span.overlaps(address, address) {
        return Some(node.span.clone());
    }
    (node.start.as_usize() <= address)
        .then(|| find_span(node.right.as_deref(), address))
        .flatten()
}

/// 与 [lo, hi] 相交的第一个关系。
fn first_overlap(root: Option<&IndexNode>, lo: usize, hi: usize) -> Option<Arc<Span>> {
    let node = root?;
    if node.max_end < lo {
        return None;
    }
    if node.left.as_ref().is_some_and(|left| left.max_end >= lo) {
        if let Some(span) = first_overlap(node.left.as_deref(), lo, hi) {
            return Some(span);
        }
    }
    if node.span.overlaps(lo, hi) {
        return Some(node.span.clone());
    }
    if node.start.as_usize() <= hi {
        first_overlap(node.right.as_deref(), lo, hi)
    } else {
        None
    }
}

fn refresh(root: &mut Option<Box<IndexNode>>, span: &Arc<Span>) -> bool {
    let Some(node) = root.as_deref_mut() else {
        return false;
    };
    if Arc::ptr_eq(&node.span, span) {
        node.update();
        return true;
    }
    if refresh(&mut node.left, span) || refresh(&mut node.right, span) {
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
    let local = node.span.active_snapshot();
    assert!(!local.is_empty(), "indexed span has no active ranges");
    assert!(
        local.windows(2).all(|pair| pair[0].1 < pair[1].0),
        "span ranges overlap or are unordered"
    );
    for (lo, hi) in &local {
        assert!(
            !overlaps_other(node, &node.span, *lo, *hi),
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
fn overlaps_other(node: &IndexNode, span: &Arc<Span>, lo: usize, hi: usize) -> bool {
    if node.max_end < lo {
        return false;
    }
    if node
        .left
        .as_ref()
        .is_some_and(|left| left.max_end >= lo && overlaps_other(left, span, lo, hi))
    {
        return true;
    }
    if !Arc::ptr_eq(&node.span, span) && node.span.overlaps(lo, hi) {
        return true;
    }
    node.start.as_usize() <= hi
        && node
            .right
            .as_ref()
            .is_some_and(|right| overlaps_other(right, span, lo, hi))
}

#[cfg(debug_assertions)]
fn audit_spans(node: Option<&IndexNode>, nest: &Nest, root: &Table, geo: Geometry) {
    let Some(node) = node else { return };
    audit_spans(node.left.as_deref(), nest, root, geo);
    let span = &node.span;
    assert!(
        span.identity.belongs_to(nest.id()),
        "indexed Span belongs to another Nest"
    );
    let state = span.state.lock();
    if let Some(source) = &state.source {
        assert!(
            !source.is_borrowed() || state.memos.published().next().is_none(),
            "borrowed Span also holds published Cells"
        );
        assert_eq!(
            source.page_count().checked_bytes(),
            Some(span.initial_size()),
            "Span source extent differs from its initial range"
        );
        assert_eq!(source.level(), span.level(), "implicit source geometry");
    }
    assert!(span.initial_size().is_multiple_of(span.grain() * PAGE_SIZE));
    for (page, cell) in state.memos.published() {
        let address = span.initial_start() + page * PAGE_SIZE;
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
        let expected = span.leaf_attributes(
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
    audit_spans(node.right.as_deref(), nest, root, geo);
}
