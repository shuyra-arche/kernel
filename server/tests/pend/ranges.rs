use super::window::Window;
use kernel_api::{
    abi::call::pend::PendFail,
    address::{PageCount, PageOffset},
    model::{
        cell::MappingId,
        pend::{NestScope, PendRange, PendScope},
    },
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
    assert_eq!(
        Window::Parent(PendRange::Inherit).resolve(parent),
        Err(PendFail::Denied)
    );
}

fn mapping(id: usize, range: PendRange) -> PendScope {
    PendScope::Nest(NestScope::Mapping {
        id: MappingId::from_bytes(&(id as u64).to_le_bytes()).unwrap(),
        range,
    })
}

#[test]
fn scope_ranges_are_parent_relative_and_observed_in_absolute_coordinates() {
    let root = mapping(
        1,
        PendRange::Pages {
            offset: 10,
            count: 20,
        },
    );
    let requested = PendRange::Pages {
        offset: 3,
        count: 5,
    };
    let child = Window::Parent(requested)
        .scope(root, mapping(1, requested))
        .unwrap();
    assert_eq!(
        child,
        mapping(
            1,
            PendRange::Pages {
                offset: 13,
                count: 5
            }
        )
    );
    let next = PendRange::Pages {
        offset: 2,
        count: 3,
    };
    assert_eq!(
        Window::Parent(next).scope(child, mapping(1, next)),
        Ok(mapping(
            1,
            PendRange::Pages {
                offset: 15,
                count: 3
            }
        ))
    );
    assert_eq!(
        Window::Parent(PendRange::Inherit).scope(child, mapping(1, PendRange::Inherit)),
        Ok(child)
    );
}

#[test]
fn ordinary_scope_resolution_cannot_select_another_mapping_or_category() {
    let map = mapping(
        1,
        PendRange::Pages {
            offset: 10,
            count: 20,
        },
    );
    let inherited = Window::Parent(PendRange::Inherit);
    for requested in [
        mapping(2, PendRange::Inherit),
        PendScope::Nest(NestScope::Whole),
        PendScope::Object,
        PendScope::Pages(PendRange::Inherit),
    ] {
        assert_eq!(inherited.scope(map, requested), Err(PendFail::Denied));
    }
    assert_eq!(
        inherited.scope(
            PendScope::Nest(NestScope::Whole),
            mapping(1, PendRange::Inherit)
        ),
        Err(PendFail::Denied)
    );
    assert_eq!(
        inherited.scope(PendScope::Object, PendScope::Object),
        Ok(PendScope::Object)
    );
}

#[test]
fn source_coordinate_override_is_internal_and_still_cannot_escape_the_parent() {
    let parent = PendScope::Pages(PendRange::Pages {
        offset: 13,
        count: 5,
    });
    let inherited = PendScope::Pages(PendRange::Inherit);
    assert_eq!(
        Window::Source(PageOffset::new(15), PageCount::new(3).unwrap()).scope(parent, inherited),
        Ok(PendScope::Pages(PendRange::Pages {
            offset: 15,
            count: 3
        }))
    );
    assert_eq!(
        Window::Source(PageOffset::new(12), PageCount::new(3).unwrap()).scope(parent, inherited),
        Err(PendFail::Denied)
    );
    assert_eq!(
        Window::Parent(PendRange::Inherit).scope(
            parent,
            PendScope::Pages(PendRange::Pages {
                offset: 0,
                count: 1
            })
        ),
        Err(PendFail::Denied)
    );
}
