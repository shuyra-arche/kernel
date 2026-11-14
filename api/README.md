# Kernel API

`kernel-api` is one `no_std` Cargo package. It owns the public Kernel contracts;
Client executes them and Server implements them. It requires no heap, startup
code or runtime state.

## Public modules

| Path | Responsibility |
| --- | --- |
| `address` | PhysAddr, VirtAddr, PageIndex, PageOffset, PageCount and page geometry constants |
| `model::identity` | TaskId, TeamId, PendId and Mark |
| `model::cell` | SlotId, allocation, Cell objects, positions, handoffs and observations |
| `model::modifiable` | Modifiable: write permission on a Pend |
| `model::unit` | User-only creation, UnitTarget, control authorities, creation descriptors, queries and stable results |
| `model::{view,pend,clone_view,sign}` | View identity, Pend state, Clone queues and Sign routing |
| `model::memory` | Bounded, atomic reads of versioned global memory observations |
| `model::{flat,nest}` | Source, Nest and ReservationId identities and observations; never independent authorization |
| `model::{wait,exit}` | Waiting and exit vocabulary |
| `abi::call` | Operation declarations, domain failures and aggregate dispatch |
| `abi::operation` | Operation, RegisterCall, KernelCall and TerminalCall traits |
| `abi::register` | Checked request packing and unpacking, including Wire and Decode |
| `abi::reply` | Shared success-register decoding traits |
| `abi::layout` | Shared little-endian framing for caller-memory descriptors |
| `abi::error` | Common failure representation and dispatch errors |
| `boot` | Initial-task arguments, Capsule, resource Ledger and initrd preamble |

The crate root exposes these four domains directly. Import names from their
owning modules. Each model type owns its checked register and byte codecs.
The Sign operation module owns its success reply and register encoding.

## Operations and generation

`tools/abi-macros` is a separate proc-macro package. Its `envcall` macro generates
only Kernel operation declarations, register codecs, dispatch enums and failure
codes. It is not a service protocol generator. Generated paths refer to
`crate::abi`; API uses this tool at compile time.

A request value contains inputs. `Operation` associates its success and error
types; `RegisterCall` defines its slot and six-word request encoding.
`KernelCall` adds pure return decoding. `TerminalCall` has no return decoder and
cannot be used with a returning invocation. Returning operations use two or
three success registers according to their declaration. Infallible operations
preserve all success bits, including the high bit.

Call classes are 16-bit and operation slots are 32-bit values. The macro checks
register capacity and rejects contradictory declarations. Errors use explicit
consecutive negative codes; an optional busy marker identifies not-ready results.
Unknown call classes, slots and invalid request fields are rejected. Packing and
decoding perform no trap, retry or rollback.

## Boot boundary

Kernel reads the initrd's initial eight-byte offset/length preamble and parses
the selected Capsule. The resource Ledger contains named initial capabilities.
Deployment selection, program catalogs, image packing and service protocols
belong to the integration owner, not this package.
## Memory geometry: three distinct units

The current public contracts use **4 KiB as their base coordinate**,
not as a universal physical allocation or page-table leaf size.

| Concept | Current representation | Contract |
| --- | --- | --- |
| ABI base page | `address::BASE_PAGE_SIZE` / `BASE_PAGE_SHIFT` (aliases `PAGE_SIZE` / `PAGE_SHIFT`) | `PageCount`, `PageIndex`, `PageOffset`, `CellPosition.page` and Slot/mapping spans count fixed 4 KiB units |
| Managed physical storage | `CellLevel`, size `2^(12 + 9*level)` | Each Cell is contiguous and aligned to its size; separate Cells need not be adjacent. Admission checks the active Sv mode and allocation checks physical capacity |
| Hardware translation leaf | Selected by each MMU/IOMMU backend | RISC-V CPU Sv39 encodes 4 KiB, 2 MiB and 1 GiB leaves; Sv48 adds 512 GiB, Sv57 adds 256 TiB. Automatic coalescing currently stops at 1 GiB. External/IOMMU admits one whole level-0 or level-1 Cell per mapping, with 4 KiB leaves |

A larger PTE leaf can cover multiple logical positions without merging their
identities, authorization, freeze or retirement records. CPU leaf changes
still require splitting and covering TLB completion; external access retains
independent trusted hardware obligations.
`Allocation::at_level(level, count)` requests complete physical Cells.
`Allocation::new(size, align)` remains level 0. Levels admitted by Sv39, Sv48 and
Sv57 are respectively 0–2, 0–3 and 0–4; representability does not imply available
RAM. Slot/mapping positions must name a Cell head; windows and transfers cover whole
Cells. ABI coordinates remain 4 KiB units, so a level-1 Cell spans 512 coordinates.

## ABI version

This revision implements ABI 11 unconditionally. Calls, native Flat/Nest
memory descriptors, Boot Ledger and Capsule ABI use the single version in
`abi::VERSION`. Capsule format remains 2. Cargo features do not select an ABI.

Pin API, Client, Server and image producers to one matching revision. Remove
`features = ["memory-v11"]` from existing dependencies when upgrading; no
replacement feature is required. Rebuild the complete image, including Capsule
and Ledger. Old Cell and Share Open/Shut calls are rejected.

ABI 10 consumers must stay on their existing immutable source revision or the
[Preview Alpha](https://github.com/shuyra-arche/kernel/releases/tag/26.11.14-preview-Alpha).
The [Preview Beta](https://github.com/shuyra-arche/kernel/releases/tag/26.11.14-preview-Beta)
is also fixed: its source still requires the old opt-in feature. Neither tag
contains this removal of ABI selection.

Call numbers are `(version << 48) | (domain << 32) | operation`. The RV64
Kernel-call transport uses `ebreak`; `a7` identifies the operation,
`a0..a5` carry arguments and `a0..a2` carry results. Supervisor-mode
`ecall` goes to SBI, not this Kernel dispatcher. Invalid call numbers,
encodings, memory references or expired authorizations are rejected; a
numeric identity or successful decode is not authority.

[Capsule format 2](src/boot/capsule.rs) requires the selected ABI in byte 11.
The [Boot Ledger](src/boot/ledger/) uses the matching ABI version with
40-byte entries. Always pin and rebuild a matching API/Client/Server,
Capsule and Ledger set; no compatibility fallback is provided.

## Callable interfaces

| Domain | Public definition | Effect owner |
| --- | --- | --- |
| Unit: Team, Task and administration | [Unit calls](src/abi/call/unit.rs), [creation descriptor](src/model/unit/creation.rs) | [Unit](../server/src/basis/unit/) |
| Room: schedule, wait and clock | [Room calls](src/abi/call/room.rs) | [Room](../server/src/basis/room/) |
| Pend: grant, attenuate and revoke | [Pend calls](src/abi/call/pend.rs), [scope](src/model/pend/scope.rs) | [Pend](../server/src/basis/pend/) |
| Clone, Share and Sign | [Clone](src/abi/call/view/clone.rs), [Share](src/abi/call/view/share.rs), [Sign](src/abi/call/sign.rs) | [View](../server/src/basis/view/), [Sign](../server/src/basis/sign/) |
| Native ABI 11 memory | [Flat/Nest descriptors](src/abi/memory/), [memory guide](memory.md) | The same Flat/Nest/Page owners |
| Debug | [Debug calls](src/abi/call/debug.rs) | [Entry](../server/src/entry/) |

The source of truth for register layouts and failure codes is the
[ABI declaration](src/abi/call/) or [native memory request](src/abi/memory/).
The [operation](src/abi/operation.rs), [register](src/abi/register.rs) and
[reply](src/abi/reply.rs) modules define checked encoding and decoding.
`tools/abi-macros` generates typed declarations, not runtime services.
The [Client guide](../client/README.md) shows how to invoke them.

## Authorization and lifecycle

**Unit.** Public `UnitCreate::Team` creates **User** Teams only and requires
Build authorization. Its 48-byte descriptor reserves word 1 as zero;
Supervisor and unknown selectors are rejected, even for a Supervisor caller.
Supervisor/Kernel boot setup remains internal. Team creation returns a
limited Whole Nest Pend; Task creation supplies no automatic mapping grant.

**Pend.** Source authorization must be live and distributable. Derived
rights, access ceilings, scope, `Modifiable` and `Distributable` can only
narrow and retain ancestor revocation. `Pend.Accord` can grant a restricted
capability to an unrelated live Task, including Dormant Tasks, without
granting control over their execution. Recipient registration is bounded
to 1,024 entries total, at most 256 incoming, 32 per sending Task and
128 per sending Team per recipient. Prepared/unsettled entries count;
exhaustion returns `OutOfMemory`. See the [delegation contract](../docs/pend-delegation.md).

**Memory.** Flat owns logical sources and physical holding positions;
Nest owns address reservations, Memo association and publication; Page
owns physical lifetime and access admission. Pend authorizes relationships,
while retained CPU/IOMMU work must settle before physical reuse.
Read the [current memory contract](memory.md), not a staged design record.

**Views and observations.** Clone copies bounded messages. Share expresses
authorized access; the selected ABI determines its operations.
An explicit `(View, Sign)` boot dispatch pair does not authorize physical
device reassignment or declare DMA quiescence. The Kernel observer publishes
read-only versioned global memory statistics via Share and Sign. Per-page
A/D readings remain `UNKNOWN`, not zero or proof of a clean Page.

## Verification

Use the [build/test commands](../README.md#build-and-test) and
[Kernel verification guide](../server/tests/README.md). Host tests verify
types and contracts; RV64 builds verify compilation; QEMU executes real
guest traps and selected hardware paths. Neither replaces physical evidence.
