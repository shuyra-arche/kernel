# Kernel documentation

Start with the [Typst design](design/main.typ) to understand how the Kernel
composes data and control across isolation boundaries and what makes those
crossings safe. Use the [source map](source-map.md) to locate the mechanisms
in the implementation.

| Reader's question | Source |
| --- | --- |
| Why does the Kernel have this model? | [Design (Typst)](design/main.typ) |
| Where does the source implement each concern? | [Source map](source-map.md) |
| What API or ABI exists today? | [Kernel API](../api/README.md) and [API source](../api/src/) |
| How are calls performed? | [Client](../client/README.md) |
| How do I reproduce verification? | [Kernel verification](../server/tests/README.md) |
| How do I build or contribute? | [Repository README](../README.md) and [Contributing](../CONTRIBUTING.md) |

## Design

The text starts with isolation, then follows a cross-domain request through
copying or borrowing, delegation, and notification. After introducing the
conditions for safe retirement, the [memory chapter](design/memory.typ)
explains Cell, Flat, Nest, and their state models. A worked example follows
one Page through association, publication, and retirement. The source map
groups implementation paths under `basis`, `entry`, and `metal`.

The design explains enduring constraints and identifies current mechanisms
where they make an argument concrete. The API owns precise call permissions,
failure results, and encodings. Issues track open questions and proposals.
PRs record the reasons for changes and their verification.

Compile the design locally with Typst:

```sh
typst compile docs/design/main.typ /tmp/shuyra-kernel.pdf
```

Keep Typst source in Git. Use the generated PDF for reading or publication.
Documentation-only changes skip Kernel builds and QEMU tests. The repository
path filters are tracked in [PR #126](https://github.com/shuyra-arche/kernel/pull/126).
