# Kernel documentation

Kernel documentation has **three distinct responsibilities**: architectural
argument, current public contract, and decision history. They should not
be maintained as competing descriptions of the same fact.

## Entry points

| Reader's question | Source |
| --- | --- |
| Why does the Kernel have this model? | [Design narrative (Typst)](design/main.typ) |
| Where does the source implement each concern? | [Source navigation](source-map.md) |
| Why was a consequential design decision made? | [RFCs](rfcs/README.md) |
| What API or ABI exists today? | [Kernel API](../api/README.md) and [API source](../api/src/) |
| How are calls performed? | [Client](../client/README.md) |
| How do mechanisms enforce the contract? | [Server source](../server/src/) |
| What was actually verified? | [Server tests](../server/tests/) and recorded CI evidence |
| How can I build or contribute? | [Repository README](../README.md) and [Contributing](../CONTRIBUTING.md) |

## Design: explain the problem before the mechanisms

`design/main.typ` assembles a coherent argument, currently in three chapters:

1. Isolation and the problem of composition.
2. Reconnecting data and control flows.
3. Safety and completion obligations.

Its chapter boundaries follow questions, consequences, and proofs, **not**
the current Rust source tree. `basis`, `entry`, and `metal` appear in the
[source map](source-map.md) only as an implementation navigation aid.

The design is a long-lived explanation of the current architectural
intent; it does not duplicate syscall numbers, field layouts, or transient
internal function names. A design principle does not claim a feature is
already implemented. Add a new chapter only when it has an independent
argument, not just to mirror a new module.

Typst source remains in Git. To render locally with Typst installed:

```sh
typst compile docs/design/main.typ /tmp/shuyra-kernel.pdf
```

Generated PDFs are publication artifacts, not source files. Do not commit
them as part of routine design edits. Document-only changes should not
launch Kernel build or QEMU CI; the path filtering for this policy is
tracked independently in [PR #126](https://github.com/shuyra-arche/kernel/pull/126).

## RFCs: preserve decisions, not duplicated specifications

See the [RFC guide](rfcs/README.md) and [template](rfcs/template.md).
Use them for consequential architecture changes with meaningful alternatives.
An RFC may be accepted without being implemented; the source code and
public contract remain authoritative about what is available now.

Issues carry work and unresolved alternatives. PRs carry the final change
rationale and verification evidence. Raw logs belong with test artifacts,
not in permanent prose.

## Rules for maintenance

- Each fact has a single enforcing or documenting owner; link rather than copy.
- Keep architectural invariants, public contracts, and implementation status
  distinct. Record uncertainty instead of silently promising a future ABI.
- When behavior changes, update its owning API contract and relevant tests.
  Update the design only if the problem model or enduring safety reasoning changes.
- Keep source-module implementation details next to their source; update the
  source map when implementation responsibilities move.
- Favor concise arguments and meaningful diagrams over empty placeholder
  sections or exhaustive inventories of implementation symbols.
