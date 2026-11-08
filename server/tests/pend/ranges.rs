use super::window::Window;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::pend::PendRange,
};

fn span(offset: usize, count: usize) -> (PageOffset, PageCount) {
    (PageOffset::new(offset), PageCount::new(count).unwrap())
}

#[test]
fn nested_offsets_are_relative_to_the_immediate_parent() {
    let root = span(10, 20);
    let child = Window::Parent(PendRange::pages(3, 5).unwrap())
        .resolve(root)
        .unwrap();
    assert_eq!(child, span(13, 5));
    assert_eq!(Window::Parent(PendRange::Inherit).resolve(child), Ok(child));
    assert_eq!(
        Window::Parent(PendRange::pages(2, 3).unwrap()).resolve(child),
        Ok(span(15, 3))
    );
    assert_eq!(
        Window::Source(PageOffset::new(15), PageCount::new(3).unwrap()).resolve(child),
        Ok(span(15, 3))
    );
}

#[test]
fn empty_expanding_and_overflowing_windows_are_denied() {
    let parent = span(13, 5);
    for (offset, count) in [(0, 0), (0, 6), (5, 1), (usize::MAX, 1), (1, usize::MAX)] {
        assert_eq!(
            Window::Parent(PendRange::Pages { offset, count }).resolve(parent),
            Err(PendFail::Denied)
        );
    }
    for (offset, count) in [(12, 1), (18, 1), (13, 6), (usize::MAX, 1)] {
        let (offset, count) = span(offset, count);
        assert_eq!(
            Window::Source(offset, count).resolve(parent),
            Err(PendFail::Denied)
        );
    }
    // The page-coordinate sum fits, but its byte coordinate does not.
    let parent = span(usize::MAX / 4096, 2);
    assert_eq!(
        Window::Parent(PendRange::Pages {
            offset: 0,
            count: 1
        })
        .resolve(parent),
        Err(PendFail::Denied)
    );
}
