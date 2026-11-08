# Kernel

Shuyra's RV64 kernel, public contracts and typed call transport.

## Layout

| Path | Responsibility |
| --- | --- |
| `api/` | One `kernel-api` package: semantic model, call ABI and boot contracts |
| `client/` | Typed calls and RV64 trap transport |
| `tools/abi-macros/` | Compile-time Kernel operation and ABI generation |
| `server/` | Kernel library and image-layout linker contract |
| `server/src/basis/` | Memory and execution mechanisms, separate from Rust `core` |
| `server/src/entry/` | Boot assembly of providers, normalized trap dispatch, calls and diagnostics |
| `server/src/metal/` | Hardware mechanisms and the selected providers |
| `server/src/metal/generic/` | Shared clock, interrupt, translation, platform ordering and trap contracts |
| `server/src/metal/allocator/` | Allocation algorithms, platform facts and allocator instances |
| `server/src/metal/riscv/` | RV64 boot handoffs, Hart state, traps and context switching |
| `server/src/metal/riscv/clock/` | Counter state and per-Hart timers |
| `server/src/metal/riscv/interrupt/` | CPU-local interrupt operations and controller claims |
| `server/src/metal/riscv/mmu/` | CPU page tables, ASIDs and TLB synchronization |
| `server/src/metal/riscv/iommu/` | Device translation, command queues and trusted completion |
| `server/sbi/` | SBI calls used by the kernel |
| `.github/workflows/` | Compilation checks and source version releases |

One root Cargo workspace and lockfile cover production packages and the Host
test package. Standalone RV64 guests keep their own workspace and lockfile.
Client and Server consume the same API; Server does not depend on Client. Language runtime/std support belongs to the separate Toolchain workstream.
Platform denotes hardware porting; Kernel defines no Runtime/Execution layer. Cluster is an organizational
boundary, not a configuration file.

`basis` owns scheduling, authorization and retirement records. The local timer
owns its confirmed comparator setting. CPU retirement accepts an exact domain-instance, attempt and coverage fence
receipt before settling Pages, tables and address tickets. External access
uses authorized Nest roots and a boot-selected port. IRQ delivery only
requests work, while the installed backend supplies retirement evidence.

## Model boundaries

Kernel reconnects data flow and control flow across Unit isolation boundaries.
The reconstruction is tracked in [#14](https://github.com/shuyra-arche/kernel/issues/14).

| Element | Responsibility |
| --- | --- |
| Cell | A range of Slots (Flat) or Memos (Nest), with position-based Page operations |
| Page | Unique physical identity, ownership epoch and CPU and external access obligations |
| Flat | Logical backing slots, residency and movement of existing Pages |
| Nest / Memo | Address organization and one authoritative per-position association/publication record |
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
instances live together in `metal/allocator`; there is no allocator adapter
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

Pend, Sign and Control keep stable public IDs while `basis/name` stores their
registrations in reusable, table-qualified generational slots. Indexes contain
keys, not copied authority records. Unpublished Pend reservations roll back on
failure; retiring records remain registered until hardware withdrawal succeeds.
Slot reuse does not restore authority or close an owned Sign endpoint. Wait
Tickets retain their non-reusing serial IDs and existing holder map.

Task control follows its explicit controller or the controlling Team ancestry;
ordinary membership does not permit controlling or reading peer Task results.
Unit Create makes an empty Team or dormant Task; Resume admits execution.
Writable cross-Unit data uses Pend Accord. Read-only code may be mapped into a
controlled constructing Team before Task creation, retaining its revocation edge.
Unit Exit records the first committed result independently of resource cleanup.
Inspect observes results repeatedly; Delete requires reclamation and removes the
caller's retained record. Sign routes completion without consuming results.
Team Finish permanently closes admission. Membership never grants peer control.

## Task register state

Tasks allocate a private FLEN=64 image on first use of a supported F/D instruction
or floating-point CSR. All 32 registers and `fcsr` are initialized, saved before
scheduling/blocking/exit and restored at the final return to the Task. Ordinary
Kernel execution keeps FS/VS disabled; only the scoped register assembly enables
FS. The fixed trap frame and one-page trampoline remain unchanged in layout.

The existing boot baseline requires RV64GC, so F-only hardware is not admitted.
A CPU advertising Q cannot enable a 64-bit Task image. Queue placement and stealing
use one compatibility check, preserving the image independently of the Hart.
This is a correctness-first full save/restore path; Dirty-state optimization is
not enabled.

For firmware-declared V, each Hart probes its hardware `vlenb` and supported SEW
set before becoming online. The supported profile is V 1.0 with SEW 8/16/32/64
and VLEN 128..65536; undeclared or unsupported profiles cannot enable VS. A Task
allocates and zeroes `32 * vlenb` bytes on first use. Whole-register operations
preserve every lane, including tails, together with `vl`, `vtype`, `vstart` and
`vcsr`. Save clears `vstart` before register stores; restore writes it last.
Vector floating instructions also acquire an F/D image when necessary.

An active vector image can move only to an exactly matching vector geometry;
QEMU covers VLEN 128/256 and absent V on one/four Harts in debug and release.
A bounded local illegal-instruction recovery protects this early probe. A false
V declaration or a trap-based unsupported-`vtype` implementation disables that
Hart's vector capability instead of crashing boot. The supported profile uses
`vill` for unsupported configurations; pre-ratification vector emulation and
trap-based configuration emulation are not provided.

Compatibility is checked at placement, dequeue/steal and final return. Placement
rotates directly within compatible idle Harts, with compatible nonlocal Harts as
the busy fallback. Gaps in the capability set do not bias the destination.
Execution ownership is asserted at save/restore. Once a Task has detached and
entered termination, its optional register images are released before asynchronous
memory cleanup or retained results finish; no Hart retains an image pointer.

The context guest checks illegal instructions, stopped running/waiting Tasks
and fresh state after repeated exits. QEMU checkpoint traces establish that each
register-bearing Task actually runs on multiple Harts. A four-Hart fixture declares
V only on physical Harts 0/1 and checks that vector execution visits both and never
2/3. Runnable load keeps eligible Harts active during 128 handoffs with varied
wakeup intervals, avoiding a fixed timer phase that can repeatedly select the
same Hart. The fixture retains QEMU's normal multithreaded execution. Mismatched
VLEN/SEW rejection and allocation-failure rollback are checked by Host tests;
QEMU runs 128/256 separately, not heterogeneous physical VLEN. Physical hardware
and measured Dirty-state optimization remain outside this verification.

## Monotonic deadlines

`Room.Clock` and `Wait::At` use nanoseconds since the Kernel captured its boot
counter. Absolute waits round upward to the next hardware tick. Overflow
saturates at the counter horizon, so an unrepresentable deadline cannot expire
early. The RV64 counter is assumed not to wrap during a boot.

The boot Hart owns the global indexed deadline heap. A changed head on another
Hart sends an IPI. The owner clears the interrupt before reading the head, so a
concurrent update is visible in the queue or leaves another notification. Each
Hart separately owns its local scheduling comparator state. An unchanged
comparator is reused, including a due comparator whose interrupt is still
pending. Timeout and explicit wake use the same commit boundary and wait ticket.

The default timer backend is SBI TIME. Trusted boot firmware may provide the
empty `/chosen/shuyra,supervisor-stimecmp` property when S-mode may access
`stimecmp` and receive supervisor timer interrupts on every admitted Hart
advertising Sstc. The Kernel uses direct CSR writes only on those Harts and
otherwise uses SBI. The ISA bit by itself is insufficient. Debug shutdown
reports comparator writes, avoided writes, measured programming ticks and
observed deadline lateness.

Trusted boot firmware can also provide the empty `/chosen/shuyra,user-time`
property when every admitted Hart exposes the same synchronized RV64 `time`
counter to both S- and U-mode, subject to `scounteren.TM`. Kernel advertises
`Room.ClockInfo` only after every admitted Hart has accepted that bit. Client
callers can cache the source in `room::MonotonicClock`, which falls back to
`Room.Clock` if discovery fails. Both paths use the same boot tick and scale.
No shared metadata page is needed.

## External access

External access is an address projection, not a separate device work object.
There is no DeviceCall, Device administrative qualification or work/device.
A boot MMIO View is delegated with Pend; Driver Units map it through Share Open
and perform their own register operations. Kernel never proxies business BAR
reads or writes.

Cell Memo creation selects MappingTarget::Cpu(TeamId) or Io(PendId). The I/O
Pend identifies a platform-bound MMIO View; a numeric address or ordinary RAM
View cannot mint a route. Source Cells use the existing Pend borrow tree and
Flat residency. The IOMMU backend owns its hardware domain, page tables and command queues;
Memo owns the payload publication obligation. Inspect
returns mapping state and the first fault; Sign routes MappingRetired and
MappingFault for that exact MappingId, rechecking target authority. Parent
holders may observe while lending, but cannot use a suspended Pend to publish.

The same Nest Memo owns the associated Page, generation, freeze, publication
and fault for CPU and external targets. Address spans own only reservations and
range policy; the ABI MappingId still names that reservation. External authority
roots retain a Nest and its Pend borrowing until hardware settlement, independently
of the creating Task. There is no separate Projection/IoSpace lifecycle. Source or target revocation, explicit Retract and
creator exit all close admission. A live external Nest retains a routing obligation on its target Pend: ordinary resource access remains
available, while reborrowing or narrowing waits for hardware settlement. Cells remain External
until the matching trusted completion settles. Notifications run after commit
and registry locks are released. An interrupt or business completion is never a
reclamation receipt. Failed synchronization retains the obligation.

Trusted boot images supply `kernel_api::boot::platform::Platform`, call unsafe
`boot::init`, then `boot::configure` with normalized physical authorities and
controller routes. `Memory::new` and `Harts::new` admit bounded facts without an
allocator; Kernel checks the executing image/ROOT reservation and current boot
hart, page-overlap rules, control ownership, source limits and hardware capabilities.
No Unit can call this Rust interface or manufacture a trusted completion provider.

The integration owner resolves firmware topology and configures the endpoint.
After protected controller setup it may call `boot::map_mmio` and
`boot::bind_external(ExternalRoute { controller, requester, base, size, proof })`.
An external resource must fit a withheld external window and must not expose RAM,
ordinary grants or protected controls. `requester` is the normalized IOMMU device
identity, not a PCI RID. `boot::start` rejects incomplete setup. Boot failure is
terminal; it does not publish partial authorities or start secondary harts.

Kernel never enables or disables PCI Bus Master, reads ECAM, sizes BARs, or uses
a device stop register during retirement. Translation withdrawal closes memory
access even if the requester continues issuing work. The RV64 backend completes
its invalidation/fences, leaves DDTP Off and disables queues before requesting
platform ordering evidence. Its opaque `Retirement` token binds the controller,
requester, binding identity and fresh retirement batch. Pending, error, and a
mismatched/stale token keep the Page and table responsibilities retained. Queue
storage has its separate confirmed shutdown boundary. The one-shot controller
cannot be rebound after settlement. No ordinary Unit can install a proof provider.
See the `Quiescence` safety contract and its pinned IOMMU specification reference;
IOFENCE PR/PW completion alone is not the platform proof.

Production board parsing and the normal `shuyra` image live in
[Vanllia platform/riscv](https://github.com/shuyra-arche/vanllia/tree/main/platform/riscv).
DTB structure, RAM/ISA strings, PLIC wiring, PCI topology, virt compatibility and
the load address all belong there. Kernel contains no Board or PCI implementation.
`server/link.x` specifies image layout and requires the image to supply
`KERNEL_LOAD_ADDRESS`; Server is a library and no longer provides a `shuyra` binary.

The EDU image and PCI tests live in
[Vanllia tests/platform/qemu](https://github.com/shuyra-arche/vanllia/tree/main/tests/platform/qemu),
using the same production platform package. Its proof leaves Bus Master enabled
and returns bounded Pending before the QEMU ordering check. The low-level hardware
fixture lives in Vanllia `tests/platform/hardware`; the standalone Kernel test
guest and Host hardware harness depend on it. Host DTB construction belongs to
that same external fixture. Production Kernel has no dependency on Vanllia or a fixture.

`KERNEL_BOOT_MANIFEST` selects the normal image and `KERNEL_PLATFORM_MANIFEST`
selects the EDU image. CI checks out a pinned consumer, patches both image manifests
to this candidate Kernel/API, updates their locks and runs locked tests. This
prevents silently testing a consumer's older provider. The source-only boot
interface replaces `init(dtp)` and `init_with_external`; images must rebuild and
supply the new descriptors. Cell ABI v1, MappingId encoding and syscall operation
numbers are unchanged; the new boot types are Rust contracts, not wire records.

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

Kernel's bounded memory constructor subtracts image/ROOT, firmware reservations,
DTB and initrd before either allocator starts. Identity mappings omit holes and
no-map pages. The DTB is an opaque resource for the initial Unit; Kernel never
parses it. Firmware pointer validity and completeness of supplied reservations are
trusted image preconditions. Separate frozen memory and CPU facts preserve the
64 KiB ROOT stack. CPU slots remain distinct from firmware IDs, with boot in slot
zero; SBI starts and remote fences use exact firmware identities.

Normalized authority validation compares page footprints while preserving original
resource names and byte sizes. It rejects ordinary grants overlapping protected
controls or external windows. PLIC contexts, interrupt lines and IOMMU register
regions are checked before MMIO effects. Firmware-specific malformed-description
regressions follow their parser to Vanllia; Kernel tests the provider admission
invariants, allocator and actual hardware lifecycle.

Use nightly Rust with `riscv64gc-unknown-none-elf`; the repository does not set a
local toolchain override. The Server package is the `kernel` library. Build `shuyra` through Vanllia's production platform manifest.

```sh
cargo +nightly fmt --all -- --check
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --all-features --target riscv64gc-unknown-none-elf
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
KERNEL_BOOT_MANIFEST=/path/to/vanllia/platform/riscv/Cargo.toml \
KERNEL_PLATFORM_MANIFEST=/path/to/vanllia/tests/platform/qemu/Cargo.toml \
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
Debug CPU and I/O boots also disable physical harts 0 and 2 in a four-hart QEMU
DTB and advertise different MMU ceilings on harts 1 and 3. These scenarios verify
a nonzero boot hart, sparse enabled identities, common Sv39 selection and the
existing scheduler/fence and IOMMU interrupt paths. Host FDT tests additionally
cover IDs above the SBI mask width; QEMU does not validate arbitrary physical
Hart ID values or physical hardware.
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
