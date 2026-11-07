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

## Model boundaries

Kernel reconnects data flow and control flow across Unit isolation boundaries.
The reconstruction is tracked in [#14](https://github.com/shuyra-arche/kernel/issues/14).

| Element | Responsibility |
| --- | --- |
| Cell | Unique physical identity, ownership epoch and CPU/device reclamation obligations |
| Flat | Logical backing slots, residency and migration of existing Cells |
| Nest / Mapping | CPU or IOMMU address projection and hardware withdrawal |
| View | Clone payload delivery or Share access to whole Cells |
| Pend | Access authority, Modifiable and the explicit borrow tree |
| Sign | Retained events, source observation, routing and waiting |
| Team / Task | Membership admission, execution control, results and eventual cleanup |

Managed Cells are currently 4 KiB RAM allocations. Flat slots and mappings retain
that same Cell; they do not create another owner. A Team ownership epoch closes
at retirement, transferring residual responsibilities to Kernel without changing
Cell identity or discharging hardware access. Reallocation requires all holders
released and all CPU/device obligations safely settled. Failed or abandoned
published obligations retain the allocation. `BorrowedRegion` represents trusted
external ranges without inventing duplicate managed Cell identities.

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
the returned Pend. Closing the queue does not revoke delivered payloads. Release
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
Build attaches a Team to the calling Task. Spawn prepares execution; writable
cross-Unit data is delivered separately with Pend Accord. Read-only code may be
mapped into a controlled Team before Spawn, retaining its Pend revocation edge.
Return stores one scalar result separately from execution exit. ReadResult keeps
it until Join consumes the receipt. Team Finish seals membership admission;
only a sealed and fully exited cohort reports Complete. Completion is not proof
that all CPU/DMA reclamation work has finished.

## DMA boundary

Kernel owns IOMMU register authority, trusted queue storage, mapping records,
withdrawal and safe Cell reuse. Driver Units configure business DMA descriptors;
business completion and FQ/PQ/CQ interrupts are not reclamation receipts.
Kernel CQ progress checks the exact fence completion and platform quiescence.
Driver exit or Pend revocation requests retirement independently of the Driver.
DMA reborrows use the same Pend tree: read-only CPU and device access may coexist;
a writable external borrow suspends its parent. Notifications follow committed
state transitions and run outside the device registry lock.

The production adapter is intentionally bounded to coherent QEMU virt with the
`qemu-edu-dma` feature, a protected EDU endpoint, one whole payload Cell and a
one-shot domain. Boot routing validates controller/requester identity and rejects
unsupported topology. ATS, PRI, PCI IOMMUs, noncoherent hardware and passthrough
are not supported by this adapter. PQ remains disabled. Unsettled hardware errors
retain storage; no timeout fabricates safe completion. The ordinary build does
not enable business DMA. Physical hardware qualification and upper service
migration remain separate work.

## Build and verification

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
when overriding RUSTFLAGS. Validate debug/release with default, semihosting,
qemu-edu-dma and all features for changes spanning conditional paths.

All test code lives under owning `tests/` trees, with `tests/<domain>/mod.rs`
entry points and Cargo declarations. Production libraries remain no_std.
Host suites cover ABI, borrowed access decisions, event state, ownership,
scripted retirement failure and boot admission. Target-only source modules are
imported into isolated host harnesses; test-only allowances cover unused parts
of these imports and never change production lint policy.

The ignored hardware suite builds a separate no_std guest and runs nine QEMU
scenarios, including expected rejection when IOMMU is missing. It uses QEMU
10.0+ (`QEMU_RISCV64` may select it) and writes commands, hashes and logs under
`server/tests/guest/iommu/target/evidence/`. It exercises real MMIO/queues and EDU
DMA, but is distinct from firmware-backed production Kernel execution.

[Vanllia](https://github.com/shuyra-arche/vanllia) owns the minimal real-Unit test:
`cargo +nightly test --locked --test device -- --ignored --nocapture`, with explicit
`SHUYRA_KERNEL_ELF`, `SHUYRA_FIRMWARE` and `QEMU_RISCV64`. Its manager/driver scenarios
cover Sign routing, cross-Unit Pend return, peer control rejection, result versus
cleanup, normal DMA, read-only DMA, driver crash and exit during activation.
No test inventory is encoded in CI; Cargo remains the test entry point.

This change intentionally breaks the old Mail/Pie ABI. See the
[API compatibility notes](api/README.md#source-migration-and-abi-compatibility).
Upper-layer migrations are deferred; pin API and Client to the same provider
revision when migrating a consumer. Kernel has no concrete Driver dependency.

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
