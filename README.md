# Kernel

Shuyra's RV64 kernel, public contracts and typed call transport.

## Layout

| Path | Responsibility |
| --- | --- |
| `api/` | One `kernel-api` package: semantic model, call ABI and boot contracts |
| `client/` | Typed calls and RV64 trap transport |
| `tools/abi-macros/` | Compile-time Kernel operation and ABI generation |
| `server/` | Kernel implementation, build script and linker script |
| `server/src/basis/` | Memory and execution mechanisms, separate from Rust `core` |
| `server/src/entry/` | Boot, traps, calls and diagnostic output |
| `server/src/machine/` | Hardware mechanisms |
| `server/src/machine/allocator/` | Allocation algorithms, platform facts and allocator instances |
| `server/src/machine/mmu/riscv/` | CPU page tables, ASIDs and TLB synchronization |
| `server/src/machine/iommu/riscv/` | Device translation, command queues and trusted completion |
| `server/sbi/` | SBI calls used by the kernel |
| `.github/workflows/` | Compilation checks and source version releases |

One root Cargo workspace and lockfile cover production packages and the Host
test package. Standalone RV64 guests keep their own workspace and lockfile.
Client and Server consume the same API; Server does not depend on Client. Language runtime/std support belongs to the separate Toolchain workstream.
Platform denotes hardware porting; Kernel defines no Runtime/Execution layer. Cluster is an organizational
boundary, not a configuration file.

## Model boundaries

Kernel reconnects data flow and control flow across Unit isolation boundaries.
The reconstruction is tracked in [#14](https://github.com/shuyra-arche/kernel/issues/14).

| Element | Responsibility |
| --- | --- |
| Cell | A range of Slots (Flat) or Memos (Nest), with position-based Page operations |
| Page | Unique physical identity, ownership epoch and CPU and external access obligations |
| Flat | Logical backing slots, residency and movement of existing Pages |
| Nest / Mapping / Memo | CPU address relations and explicit per-position publication state |
| View | Clone payload delivery or Share access to whole Cells |
| Pend | Access authority, Modifiable and the explicit borrow tree |
| Sign | Retained events, source observation, routing and waiting |
| Team / Task | Membership admission, execution control, results and eventual cleanup |

Managed Pages are currently 4 KiB RAM allocations. Flat slots and Memos retain
that same Page; they do not create another owner. A Team ownership epoch closes
at retirement, transferring residual responsibilities to Kernel without changing
Page identity or discharging hardware access. Reallocation requires all holders
released and all CPU and external obligations safely settled. Failed or abandoned
published obligations retain the allocation. `BorrowedRegion` represents trusted
external ranges without inventing duplicate managed Page identities.

`basis` contains `cell`, `flat`, `nest`, `view`, `pend`, `sign`, `unit`
and `room`. Flat and Nest are peers. Each Team holds its Nest directly. Nest owns address
windows, reservations and retirement tickets. Occupancy releases an address
only after the covering translation invalidation has completed. The allocator implementation and its
instances live together in `machine/allocator`; there is no allocator adapter
package. `api::address` owns address/page-coordinate types; machine-dependent
canonicalization stays in the CPU MMU backend.

Cell exposes Create/Delete, Push/Pull, Publish/Retract, Protect and Inspect.
Push/Pull snapshot a CellHandoff and only move a Slot holding or establish a
Memo association; publication is explicit. A Memo retains the exact Page even
when the original Slot moves. Vacant, Ready and Published are separate stable
states; hardware publication and withdrawal have retained in-flight states.
Missing Slots never refill with zeroes. Replacement uses a qualified association,
not a fault that silently follows the newest Slot Page.

Retract closes Page admission before withdrawing hardware and keeps a freeze in
the logical Memo after fencing. Another Memo cannot bypass that freeze to open a
conflicting writer. Read-only external access can preserve the snapshot. Non-wrapping Memo
generations reject stale publication, replacement and protection. Delete and
replacement explicitly release that object's freeze; historical references do
not keep deleted freezes alive. Begin/End and public change receipts are gone.

Swap policy remains a Page-service responsibility. Kernel supplies stable Memo
identity, exact Page association, freezing, ownership movement and trusted
retirement. `Internal` / `External` describe absence/presence of retained external
access obligations independently of ownership, Slot location or CPU mappings.
See [the ABI contracts](api/README.md#cell-contracts) for the full endpoint matrix.

A Share View has a single Pend tree. Modifiable is its only data access flag:
readers coexist, a writer excludes all other borrowers, and a mutable parent
stays suspended until every child returns. The parent flag is not narrowed by
reborrowing. CPU mappings are withdrawn before lending; returning access waits
for actual hardware settlement. Revocation retains a Retiring Pend on failure
and Task cleanup retries it. Windows contain whole 4 KiB Cells; this initial
borrow model conservatively suspends the entire parent even for a narrower child.
Larger physical Cells and parallel disjoint writers are not implemented.

Clone Push copies bytes once into immutable, zero-filled Kernel-owned Cells.
Pull(queue, max_len) returns a separate read-only payload Pend, its exact byte
length and sender. Opening that Pend maps the existing Cells, with zeroed padding
up to the page boundary; receiving performs no second data copy. The payload
uses Share access to the copy, not the sender's original memory. A failed Pull
retains the message, including when max_len is too small; Peek reports its size.
Publication and dequeue commit together. A later Open failure can be retried on
the returned Pend. Closing the queue does not revoke delivered payloads. Pend Return
or Task exit withdraws their mappings and holds before Cell reclamation.

This replaces the copy-out Pull ABI: slot 3 is retired, mapped Pull uses slot 8,
and Oversize is removed. Callers must rebuild and release received payload Pends.
Same-Team Tasks share an address space; trusted language wrappers must synchronize
ordinary concurrent memory access. Pend does not create hardware isolation
between Tasks in that same address space.

Control authority uses separate ControlId handles, never data Pend flags. Sign
has explicit Listen/Trigger/Route rights, delegated subsets, close/release and
source authority rechecks. A pending trigger remains until consumed; another
trigger returns Busy instead of overwriting it. Source readiness is retained,
including registration after completion. View-local signal bits are removed.

Task control follows its explicit controller or the controlling Team ancestry;
ordinary membership does not permit controlling or reading peer Task results.
Unit Create makes an empty Team or dormant Task; Resume admits execution.
Writable cross-Unit data uses Pend Accord. Read-only code may be mapped into a
controlled constructing Team before Task creation, retaining its revocation edge.
Unit Exit records the first committed result independently of resource cleanup.
Inspect observes results repeatedly; Delete requires reclamation and removes the
caller's retained record. Sign routes completion without consuming results.
Team Finish permanently closes admission. Membership never grants peer control.

## External access

External access is an address projection, not a separate device work object.
There is no DeviceCall, Device administrative qualification or work/device.
A boot MMIO View is delegated with Pend; Driver Units map it through Share Open
and perform their own register operations. Kernel never proxies business BAR
reads or writes.

Cell Memo creation selects MappingTarget::Cpu(TeamId) or Io(PendId). The I/O
Pend identifies a platform-bound MMIO View; a numeric address or ordinary RAM
View cannot mint a route. Source Cells use the existing Pend borrow tree and
Flat residency. The IOMMU backend retains translation and fence obligations. Inspect
returns mapping state and the first fault; Sign routes MappingRetired and
MappingFault for that exact MappingId, rechecking target authority. Parent
holders may observe while lending, but cannot use a suspended Pend to publish.

Cell projection records join Pend obligations to trusted backend completion.
They survive execution exit. Source or target revocation, explicit Retract and
creator exit all close admission. A live I/O projection retains a routing obligation on its target Pend: ordinary BAR access remains
available, while reborrowing or narrowing waits for hardware settlement. Cells remain External
until the matching trusted completion settles. Notifications run after commit
and registry locks are released. An interrupt or business completion is never a
reclamation receipt. Failed synchronization retains the obligation.

Trusted boot images supply the exact PCI route, BAR placement and quiescence
check. The Kernel validates the route and keeps ECAM, IOMMU registers, private
queues and interrupts under its control. It exports the validated BAR as a Pend.
The normal boot image supplies no PCI route. Upper Units own general PCI policy.

The test image in `server/tests/platform/qemu` supplies the coherent QEMU virt
EDU route. It exercises one full payload Cell, exact IOVA and a one-shot domain.
ATS, PRI, PCI IOMMUs, noncoherent hardware and passthrough remain unsupported.
PQ is disabled.
Settled logical records survive for observation/Delete while their creator lives.
After creator death they expire only when hardware responsibility has settled.

ABI v1 rejects version-zero calls and removed domains. Capsule v2 declares the
ABI before execution; Ledger v3 removes obsolete Doom/Retain qualifications.
Cell uses fixed descriptor snapshots and generation-qualified transitions.
Protect is CPU-only; a published I/O binding cannot widen access. The current
one-shot platform route rejects republishing after settlement. Upper consumers
must rebuild and migrate separately; this provider change does not update them.

## Build and verification

Boot validates all enabled root RAM nodes and `reg` tuples before initializing
either heap. It subtracts the kernel/ROOT stack, DTB, initrd, FDT reservation
map and static `/reserved-memory` regions. Bump uses the largest remaining
interval; Frame admits every remaining interval after excluding Bump's consumed
prefix. Identity mappings omit bank holes and `no-map` pages. MMIO exclusion
continues to conservatively protect the whole RAM envelope.

Early parsing uses fixed storage: at most 16 disjoint RAM ranges, 64 merged
reservations (including boot objects) and 32 tree levels. Exhaustion, conflicting
boot objects, overflowing or malformed descriptions fail boot before allocation.
RAM and DTB addresses must be page aligned; partial reserved pages are withheld.
Dynamic reserved-memory requests and nonempty/missing reserved-memory `ranges`
are rejected. Allocator metadata still indexes the physical envelope, so widely
separated banks can exhaust the bootstrap interval; this reports allocation
failure rather than admitting holes. The firmware must supply a readable FDT
blob; structural validation cannot establish the validity of a raw pointer.

Use nightly Rust with `riscv64gc-unknown-none-elf`; the repository does not set a
local toolchain override. The Server package is `kernel`, its executable `shuyra`.

```sh
cargo +nightly fmt --all -- --check
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --all-features --target riscv64gc-unknown-none-elf
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
cargo +nightly test --locked -p kernel-memory-tests --test hardware --target x86_64-unknown-linux-gnu -- --ignored --nocapture
```

RV64 production builds enforce `-Dwarnings` without blanket unused/dead-code
allowances. Keep the configured relocation, frame-pointer and code-model flags
when overriding RUSTFLAGS. Validate debug/release with default and all features
for changes spanning conditional paths.

All test code lives under owning `tests/` trees, with `tests/<domain>/mod.rs`
entry points and Cargo declarations. Production libraries remain no_std.
Host suites cover ABI, borrowed access decisions, event state, ownership,
scripted retirement failure and boot admission. Target-only source modules are
imported into isolated host harnesses; test-only allowances cover unused parts
of these imports and never change production lint policy.

The ignored hardware suite builds an initial Unit against API/Client, boots the
actual Kernel through Capsule, and runs debug with one/four harts and release
with four harts. An additional debug/four-hart CPU boot supplies two RAM banks,
a physical hole and reservation-map entries through a modified QEMU DTB.
CPU scenarios cover all Slot/Memo endpoint shapes, stale
transitions, persistent freezing, replacement, borrowed ownership denial, Pend
windows, Sign revocation, stable Unit results and atomic WaitValue/WakeValue.
The I/O variants boot the separate test image with the same Kernel library and
exercise Cell traps against QEMU EDU/IOMMU. They cover DMA permission failure,
trusted retirement, ownership movement and creator death with a delayed callback.
Each variant preserves its own commands, hashes and logs under `server/tests/guest/cell/target/evidence/`.

The separate no_std IOMMU guest runs ten hardware scenarios covering queues,
failure, delayed completion, owner exit and denied DMA after physical reuse. It
uses production hardware mechanisms directly and is distinct from the real Kernel
call tests. Its evidence is under `server/tests/guest/iommu/target/evidence/`.
`QEMU_RISCV64` selects QEMU 10.0+; `QEMU_OPENSBI` selects firmware for Kernel tests.
Physical-device quiescence, ATS/PRI and noncoherent DMA are not validated here.

Tests remain Cargo entry points; CI does not encode a scenario inventory.
Upper-layer migrations are deferred. Pin API and Client to the same provider
revision, and regenerate boot artifacts according to the
[ABI compatibility notes](api/README.md#source-migration-and-abi-compatibility).
Kernel has no concrete Driver dependency.

## Versions

The repository has one source version in `[workspace.package].version`; every
package inherits it. API, Client and Server are released together at one Git
commit. A release records ABI/API compatibility changes and relevant validation.
Consumers should pin a release tag or exact revision, coordinating a single
canonical API source across their dependency graph.

To release, update the root version and regenerate `Cargo.lock`, merge through a
PR, then push a matching `d<version>` tag such as `d0.0.1` on that merged commit. The release
workflow checks the version, main-branch ancestry and builds, then creates a
GitHub Release with generated change notes. Tags with a prerelease suffix create
prereleases. Release notes can be edited to explain compatibility changes.

Releases identify source versions. They upload no ELF, binary or image; GitHub
provides its normal source archives. A Cargo version declaration is not itself a
published release, and the workflow does not publish packages to crates.io.

## Source

Kernel and API derive from [anreonyr/sqware](https://github.com/anreonyr/sqware)
commit `672d8d2deafc8067c94b6af340a5f89c6a765c9c`. Historical Client mechanisms derive from
[Execution](https://github.com/shuyra-arche/execution) commit
`1b333a28bc4c34beaea3f7986a9bb6e8ab2db414`. SBI was imported without source changes
from [SBI](https://github.com/shuyra-arche/sbi) commit
`2dc10bafb919eb2e8f6ca37c95cfd56f9a05831d`, originally extracted from
`anreonyr/sqware` at `c970afaf7dae1145633278253351be729b625717` (`crates/sbi`).

Original author identities and migration records remain in Git history. The
source checkpoints contained no license grant; this repository adds none.
Preserve third-party notices with their code.

Room conditional waits (`WaitValue` / `WakeValue`) compare a resident, naturally
aligned atomic `u32` and enqueue under the same commit lock as wake. Queues are
private to an address space; wake removes one or all current waiters and never
stores a notification. Timeout, task death and space retirement use the existing
wait-ticket cleanup. Callers keep the object and mapping alive and recheck their
condition after a successful wait. Concurrent mapping transactions report `Busy`.
