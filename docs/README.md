# Kernel documentation

| Reader's question | Source |
| --- | --- |
| How do execution, data delivery and authorization fit together? | [Unit, Room and Kernel Work](architecture/unit-room-work.md) |
| How do Cell, Flat and Nest compose and retire access? | [Target memory architecture](design/memory.typ) |
| What API or ABI exists today? | [Kernel API](../api/README.md) and [API source](../api/src/) |
| What does the selected Flat/Nest memory ABI provide? | [Current memory contract](../api/memory.md) |
| How is per-page activity authorized? | [CPU activity](../api/memory.md#authorized-cpu-activity) |
| How are calls performed? | [Client](../client/README.md) |
| How do I reproduce verification? | [Kernel verification](../server/tests/README.md) |
| How do I build or contribute? | [Repository README](../README.md) and [Contributing](../CONTRIBUTING.md) |

## Design

The [memory chapter](design/memory.typ) derives the target Cell, Flat and Nest
contracts from ownership, logical position and address access. It describes the
CPU and I/O translation backends, safe table-page retirement, and a worked DMA
example. The [current memory contract](../api/memory.md) describes ABI 14.

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
