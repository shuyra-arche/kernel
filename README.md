# Kernel

Shuyra's RV64 kernel, public contracts and typed call transport.

## Layout

| Path | Responsibility |
| --- | --- |
| `api/` | One `kernel-api` package: semantic model, call ABI and boot contracts |
| `client/` | Typed calls and RV64 trap transport |
| `tools/abi-macros/` | Compile-time Kernel operation and ABI generation |
| `server/` | Kernel implementation, build script and linker script |
| `server/allocator/` | Physical allocation mechanisms |
| `server/sbi/` | SBI calls used by the kernel |
| `.github/workflows/` | Compilation checks and source version releases |

One root Cargo workspace and lockfile cover production packages and the Host
test package. Standalone RV64 guests keep their own workspace and lockfile.
Client and Server consume the same API; Server does not depend on Client. Language runtime/std support belongs to the separate Toolchain workstream.
Platform denotes hardware porting; Kernel defines no Runtime/Execution layer. Cluster is an organizational
boundary, not a configuration file.

## Control-flow boundaries

Kernel rebuild work is tracked in [#14](https://github.com/shuyra-arche/kernel/issues/14).
The model starts with data and control flows crossing isolation boundaries.
Under Server's `core/work`, `unit` owns execution control, lifecycle and results;
`sign` owns notification sources, wait registration and routing; `room` owns
scheduling and execution handoff. A suspended operation stays with its Task;
Join results and memory conditions are interpreted by their owning domains.
Team/Task pause and resume decisions use explicit state transitions before
applying scheduling effects under the commit boundary.

This first migration keeps the existing public calls, notification policies and
source keys. It does not yet implement the new Cell/View/Pend model or public
Sign objects and reliable-event delivery.

## Memory boundaries

`Flat` organizes backing slots, residency and frame transfer; `Nest::Mapping`
projects a Flat range into an address space. Mapping references retain frames
while hardware can still access them; they do not create independent physical
ownership. Permission changes compute the next logical ranges and PTE update
intents before Nest prepares and commits hardware effects. Pending invalidation
and retained resources remain explicit until shootdown succeeds.

The Server mapping types, source index, Space registry and wait keys use
`Mapping` to distinguish address projection from the planned cross-Unit `View`.
The current public ABI still names mapping handles `ViewId`, `ViewSelection` and
`ViewControl`; these are legacy mapping contracts, not the new interaction model.
Allocator-managed 4 KiB RAM has a `Cell`: it consumes the unique allocation
receipt and assigns a non-reused instance identity. Flat consumes unique Cells
when creating slots; each managed slot holds an `Arc<Cell>`. Mapping publication
and retirement retain that same Cell through a non-cloneable hardware obligation.
There is no intermediate Frame
resource or separate Custody state. A retained `Arc<Cell>` cannot be imported
into another Flat; sharing uses ranges of the existing Flat. Moving/Reserved
slots and the Flat locks govern transfer admission, while Nest governs hardware
withdrawal and settlement. Transfer does not create a new Cell or change its ID.

Managed CPU mappings register a Cell obligation before publishing PTEs. A
prepared obligation can be cancelled; after publication it must be retired and
explicitly settled after successful shootdown. Failed invalidation keeps the
records for retry, including global scope when a batch changes G entries.
Dropping an unsettled published obligation permanently retains the Cell and
allocation and rejects new registrations. This allocation-free fallback is a
leak, not a recoverable quarantine queue. Cell destruction returns memory only
when its hardware ledger is clear; ordinary references must also have ended.
Cell now accounts for CPU and device obligations separately in one atomic state.
Its Internal/External exposure reflects outstanding device obligations, including
unpublished preparations and failed retirements; returning to Internal does not
discharge CPU obligations or ordinary holders.

The internal `Nest::iommu::IoSpace` protocol registers a whole managed Cell before
device publication, retains its projection, and associates each withdrawal with
a non-reused completion batch. Closing seals admission and preserves failed work
for retry; an old or foreign completion cannot settle a new attempt. Hardware
effects are supplied through a trusted unsafe adapter contract that includes
platform-specific synchronization, not merely a CQ completion flag. A polling
command engine is installed for coherent QEMU virt, but no complete device-projection
adapter, runtime device binding, Unit-exit hook or PQ/CQ/FQ interrupt routing is installed
yet, and the current user-space IOMMU service has not been migrated. This remains the
External protocol milestone, not protection of currently running DMA. Registration
also requires the caller to uphold borrowing and Flat residency; the protocol does
not implement Pend authorization.

Boot device grants are now computed as a complete plan before any device authority
is created. The planner reserves FDT `riscv,iommu` register pages, including disabled
nodes, and withholds every ordinary region or PCI aperture whose page-rounded
mapping overlaps them. It withholds an entire overlapping source rather than
inventing fragment resource names. Ordinary device regions cannot alias managed
DRAM; PCI apertures overlapping DRAM are rejected. Controller discovery currently
supports physical `reg` coordinates at the root or through explicitly identity
mapped buses. Malformed controller ranges, translated buses and declared unknown
or PCI IOMMUs fail boot admission. Unlisted PCI devices are not enumerated here.

This intentionally removes System's raw `platform/iommu` register supply. The old
IOMMU Unit and deployments requiring that supply cannot run unchanged. Nonoverlapping
ECAM and PCI apertures retain their existing boot resource names for the current
platform-IOMMU topology; this does not secure the movable BAR/configuration of a PCI
IOMMU. Device mapping/settlement and consumer migration are
still required before an IOMMU deployment can run with Kernel-managed DMA. Merely
withholding register authority neither disables existing DMA nor proves reclamation.

On QEMU virt, boot now requires DDTP Off and idle queues, maps the reserved register
page only into Kernel, and retains three ordinary Cells for CQ, FQ and completion
storage. All three register External obligations before publishing queue addresses.
The command engine executes one tagged invalidation/fence batch at a time; completion
requires the matching write and the CQ head past that fence. Shutdown requires every
submitted batch completed and both queues off/not busy before settling the Cells.
Failure or abandonment retains published storage; no reset/recovery path is supplied
yet. Polling steps are bounded, and bootstrap/shutdown use finite step budgets.
DDTP stays Off, PQ and queue interrupts stay disabled. CQ success is not a device-DMA
reclamation proof. Other hardware needs an explicit coherence and ordering adapter.

An internal, one-shot `riscv::Domain` now composes that engine with a one-level
DDT, three Sv39 page-table Cells and one whole payload Cell. It is deliberately
bounded to one device and one 4 KiB mapping; it is not a new public resource or a
replacement for IoSpace/View/Pend. Before DDTP publication it retains every page
and clears old directory/translation caches while Off. Only the chosen device
context is valid; ATS/PRI/MSI translation remain disabled. Withdrawal clears the
leaf, invalidates directory and S-stage caches, and waits for the tagged fence.
Closing additionally returns DDTP to Off, invalidates again and shuts down queues.

`DomainPhase::Closed` is not `SafeToReclaim`: all five table/payload obligations
remain held until a separate unsafe Kernel-only settlement step is supplied a
platform quiescence proof. Dropping even a Closed domain without that proof retains
them. There is no Unit Ack or FQ-based release. Normal Kernel boot still leaves DMA
Off; Domain is exercised by the isolated guest, and runtime device discovery,
IoSpace adapter composition, Pend authorization, interrupts and Unit-exit cleanup
remain future work. This proves a bounded hardware mechanism, not the assembled
Vanllia DMA service replacement.

Public Memory Allocation requests must be at least 4096 bytes. Smaller requests are
rejected at decode and Server admission; valid lengths are rounded up with checked
arithmetic (4097 becomes 8192). An allocation can span multiple noncontiguous Cells.
This restriction does not apply to Kernel's internal heap. Unit-local small objects
remain the runtime/Talc allocator's responsibility. No Private/Shared Cell types or
alternate physical ownership are introduced. The new Share/Pend whole-Cell contract
and larger Cell sizes remain work in [#22](https://github.com/shuyra-arche/kernel/issues/22).

Unit owner assignment/transfer, larger Cells, platform DMA integration and
registration of externally supplied physical ranges remain in #14.
`BorrowedRegion` still represents caller-retained physical ranges, including
bootstrap aliases and repeated trampoline mappings; importing each such range as
an independent Cell would create duplicate identities. Neither Cell identity nor
reference counts alone prove exclusive access or hardware quiescence.

## Build

Kernel uses nightly Rust for `allocator_api`. The repository does not impose a
local toolchain override. CI installs the latest nightly without pinning a date.
To install or update nightly and build locally, run from the repository root:

```sh
rustup toolchain install nightly --profile minimal --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --target riscv64gc-unknown-none-elf
cargo +nightly build --locked -p kernel --all-features --target riscv64gc-unknown-none-elf
```

The Server package is still named `kernel`; its executable is still `shuyra`.
Its manifest is now `server/Cargo.toml`. Build output remains under the root
`target/`. The optional `semihosting` feature exports crash reports. Normal
assertions, lock-order checks and crash diagnostics remain available.

Run all Host suites through Cargo's workspace selection:

```sh
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
```

The explicit Host target overrides this repository's RV64 build default. Substitute
your Host triple on other platforms. `server/tests/` is a test-only member of the
root workspace and shares its lockfile; it does not depend on the RV64 Server
crate. Targets without Host tests keep their test/doctest harnesses disabled.
The standalone no_std guests under `server/tests/guest/` remain separate workspaces.
Adding a case uses `#[test]`; adding a suite uses its owning Cargo manifest, not CI.
There is no additional verification CLI or orchestration package.

Tests, compile-fail fixtures, expected diagnostics and test support stay under
each owning package's `tests/` tree, grouped by ABI, boot, call or declaration
behavior. Suite entry points use `tests/<domain>/mod.rs` and explicit Cargo
`[[test]]` declarations. The production libraries remain unconditionally
`no_std`; host integration tests use `std`. `trybuild` is a test-only dependency for public
macro and operation type contracts. Library doctests are disabled.

Host call checks exercise the public `invoke_with` boundary with a recording
transport; they do not recompile private wrappers or perform native traps. Boot and system
integration validation belong to [Vanllia](https://github.com/shuyra-arche/vanllia),
which assembles the selected Units. Compilation alone does not establish guest
execution or resource reclamation.

The isolated host harness under `server/tests/` compiles the production Cell
and slot-admission modules directly, with a bounded test allocator to observe
physical-address reuse and return counts. It does not load the RV64-only Server
crate or mock Cell behavior. These checks cover instance identity, allocation
lifetime, consuming import, hardware-obligation retention and concurrent
settlement bookkeeping. They do not establish full Flat/Nest integration,
transfer races, actual TLB/DMA completion or Unit ownership.

The IOMMU host suite runs the production Cell and IoSpace with scripted hardware
effects. It checks ordering, partial publication failure, closure, retry, stale
completion rejection, independent CPU/device lifetimes and physical return counts.
RISC-V command encoding and CQ completion samples are checked separately; these
samples alone never authorize Cell settlement. The fixture cannot establish that
an actual hardware adapter meets its unsafe contract.

The boot host suite feeds encoded DTBs through the production parser and admission
planner. It checks controller aliases, page rounding, PCI windows, malformed and
unsupported discovery, RAM exclusion and unchanged names for unrelated resources.
It does not execute boot authority publication or establish hardware isolation.

The isolated RV64 hardware guest lives under `server/tests/guest/iommu/`. It compiles
the production Cell, command engine and MMIO adapter, using its own bounded allocator,
M-mode entry and failure reporter. It executes 300 real CQ batches (including ring
wrap), checks no premature return, closes both queues, and reuses a page with a new
Cell identity and zero contents. A second run abandons an active engine and verifies
that all three allocations remain retained; a missing-IOMMU run must fail. These
command-engine scenarios do not by themselves test endpoint DMA. Three additional
guest scenarios use two QEMU EDU PCI functions: authorized bidirectional DMA and revoke/reuse, read-only
protection on cold and cached translations, and active-domain abandonment.
They reject a foreign device ID and a physical-address alias; after withdrawal,
old IOVA writes cannot alter the new Cell at the reused physical address. The
fixture performs synchronous PCI reads after teardown as its platform drain;
EDU has no ATS and QEMU completes its emulated DMA calls synchronously. This
platform-specific proof is test-owned, not a production device recovery path.
No IRQs, noncoherent hardware or cross-Unit cleanup are tested.

With QEMU RISC-V supporting `virt,iommu-sys=on` (validated on 10.0.13), run:

```sh
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu \
  -- --ignored --nocapture
```

`QEMU_RISCV64` can select a local executable. The runner builds the release guest,
uses no external firmware, imposes a 30-second deadline, and saves QEMU version,
guest SHA-256, commands and serial evidence under the guest's ignored `target/evidence/`.
The CI guest job installs QEMU in Debian trixie and runs the ignored workspace
tests; the current hardware suite executes all six scenarios. Ordinary Host
tests keep it ignored. CI selects execution environments, not packages or suites,
and uploads `guest-evidence` from the guest evidence directories. Firmware-backed
production boot and assembled Unit/DMA lifecycle acceptance remain distinct from this isolated guest.

Generic byte codecs and service-protocol macros are no longer part of Kernel.
The [API migration notes](api/README.md#source-migration-and-abi-compatibility) list removed
source exports and preserved ABI layouts. Kernel has no Weave dependency.

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
