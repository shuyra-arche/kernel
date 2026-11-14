# Kernel Client

`kernel-client` executes the public [Kernel API](../api/README.md) from
ordinary Kernel Tasks. It is a `no_std` typed call library, not a
runtime, service implementation, TLS registry or ownership manager.

## Image and transport

API, Client and Server implement ABI/Ledger 11 with Capsule format 2.
No ABI feature is needed. Use [`memory::{flat,nest}`](src/memory.rs) for memory
operations and the accepted non-memory [`call::*`](src/call/) interfaces.
Old Cell and Share Open/Shut operations are rejected by the provider.

Pin all image components to one matching revision and rebuild Capsule and Ledger.
When upgrading from an opt-in native build, remove `features = ["memory-v11"]`
from the dependencies. ABI 10 consumers must retain their old immutable pin
until they migrate. Existing release tags retain their original behavior.

[`invoke_with`](src/invoke.rs) packs once, invokes a supplied test
transport once and decodes once. `unsafe invoke` uses the native
[RV64 transport](src/transport.rs): `ebreak` with `a7` as the
operation number, `a0..a5` as inputs and `a0..a2` as outputs.
S-mode `ecall` belongs to SBI, not this dispatcher. A terminal
`Unit.Exit` does not return normally.

For native memory, [`memory::invoke_with`](src/memory.rs) uses checked
fixed-size request/reply buffers and validates response correlation;
`memory::flat::invoke` and `memory::nest::invoke` execute selected
calls. A reply delivery error or a missing output **does not prove
rollback**: use the retained `RequestId` and observe its original
result rather than issuing a fresh mutation. No call implicitly
retries, creates permissions, transfers Page ownership or releases
hardware obligations.

## Authority and lifecycle

Public `UnitCreate::Team` builds **User** Teams only, with Build
authority. Its 48-byte Team descriptor has a reserved-zero privilege
word; Supervisor requests are rejected before publication.
`CreatedUnit::Team { team, nest }` returns the limited Whole-Nest
Pend. Task creation grants no ambient memory mapping permission.
Exit may target the Task, or a Team explicitly permitting member exit.

`Pend.Accord` derives from a live, distributable source and can
grant an unrelated active or Dormant Task. This gives neither
management authority nor extra resource access; the child retains
scope/data ceilings, ancestor revocation and fixed recipient
budgets. See [Pend](../api/src/abi/call/pend.rs) and its
[registration rules](../docs/pend-delegation.md).

Named Ledger entries identify initial capabilities; the actual Pend
authorizes them. `Entry::dispatch()` returns a distinct `(View, Sign)`
pair. Sign delivery, memory publication and verified CPU/IOMMU
retirement are different events. The read-only global memory
snapshot reports per-page A/D as `UNKNOWN` rather than assuming
cleanliness.

Read the [current memory contract](../api/memory.md) and
[Boot contracts](../api/src/boot/) for lifetime and version rules.

## Verification

Run from the repository root:

```sh
cargo +nightly fmt --all -- --check
cargo +nightly test --locked --workspace --target x86_64-unknown-linux-gnu
cargo +nightly build --locked --workspace --target riscv64gc-unknown-none-elf
cargo +nightly build --locked --workspace --release --target riscv64gc-unknown-none-elf
```

Host tests validate typed transport and codecs, not real privileged
instructions. Follow [target verification](../server/tests/README.md)
for actual RV64/QEMU integration and its physical-hardware limits.
