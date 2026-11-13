# {{RELEASE_TAG}}

**Preview Alpha — not stable.** Source revision: `{{RELEASE_COMMIT}}`.
This is the first public Kernel source preview. It is not a complete Vanllia or
System product release, a Cargo registry publication, or a physical-hardware
qualification.

## Selected contract and migration

| Item | Selection |
| --- | --- |
| Kernel call ABI / Boot Ledger | 10 / 10 (default feature selection) |
| Boot Capsule | Format 2; ABI byte 10 |
| `kernel`, `kernel-api`, `kernel-client`, `kernel-api-macros` | 0.0.1, `publish = false` |
| SDK pins | API and Client from the same full release commit or immutable tag |

Rebuild executables, Capsule descriptors and Ledger consumers together. ABI 9
binaries and future ABI declarations are rejected. Cell allocation creates a
Slot; Nest.Reserve uses an actual Whole-Nest Pend, then generation-qualified
Insert and Publish establish associations and hardware visibility. Task/Team,
Nest and mapping identifiers describe objects; they do not grant authority.
See the [API migration](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/api/README.md#source-migration-and-abi-compatibility)
and [Client guide](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/client/README.md).

## Changes

- General Cell transfer, association and hardware-publication operations retain
  separate permission and failure contracts. Live Pend ancestry bounds rights,
  windows, data access, redistribution, revocation and cleanup.
- Nest retirement prepares all cuts and non-reused ticket IDs before mutation.
  One invalidation attempt owns one exact fence proof. Contention returns Busy;
  cleanup retains pages and retirement responsibility until proof settles.
- IOMMU fault injection covers queue writes, tail publication and lost writes.
  Partial submissions retain payload, page tables and queues. Late completion
  cannot revive a failed owner or replace platform Quiescence/Binding proof.
- Pend.Accord supports unrelated Tasks and Dormant recipients. A recipient has
  1,024 registration slots including preparation, at most 256 incoming, at most
  32 from one sending Task and 128 from one sending Team. Incoming saturation
  leaves 768 slots for local work. Capacity failure is OutOfMemory. Return,
  Revoke and exit release charges after cleanup settles. Unit management rights
  remain independent. This policy bounds resource use without promising sender
  fairness under incoming saturation.
- A schedulable Kernel observer publishes versioned, consistent global memory
  facts through read-only Share and Sign. The snapshot cannot authorize access
  or prove physical reclamation; consumers receive no arbitrary physical address,
  raw PTE or unauthorized per-Unit page state.

The retirement and delegation ownership records are
[here](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/docs/nest-retirement.md), [here](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/docs/iommu-failure.md) and
[here](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/docs/pend-delegation.md). PRs #162, #163 and #164 contain focused reviews
and pre-merge validation.

## Verification and limits

The release workflow runs locked host tests, ABI-selection checks, RV64 debug
and release builds, the native provider/client checks and all six QEMU shards
on this exact revision before publishing. See the release's Check run and
`guest-evidence-*` artifacts for commands, compiler identity, ELF hashes and
serial evidence. Formatting and the reproducible local commands are documented
in the [verification guide](https://github.com/shuyra-arche/kernel/blob/{{RELEASE_COMMIT}}/server/tests/README.md).

The source candidate uses Rust nightly 1.102.0 (`8eb778d04094d3af4b26580cab327e0413eb1e02`),
Linux x86-64 host tools, QEMU 10.0.13 and OpenSBI 1.6. The pinned Vanllia fixture is
`eb37b0b0db299513274864cec23cee509a3cd3cc`; its two manifests are patched only to
bind the platform image to the Kernel revision under test. Host failure tests,
RV64 compilation and QEMU execution provide distinct evidence. QEMU does not
prove physical DMA isolation, instruction-cache coherence or real-controller
failure behavior.

Twine's Share guest and System's native component guest validate one/four-Hart
operation. The C/S/U case delivers S -> Dormant U initial authority and U -> S
reply authority with actual Kernel sender identities, rejects a mismatched
reply requester, denies S's execution control over U, and checks results and
reclamation. Their immutable consumer revisions and commands are recorded in
the release preparation PR. This does not establish an assembled System boot;
its wider native migration remains draft.

Per-page accessed/dirty sampling is **Unknown/Unsupported** (#139). The optional
native memory-v11 selection and its remaining end-to-end acceptance are tracked
by #142 and are not the default preview contract. Swap, drivers, filesystems,
full Toolchain integration and physical-hardware qualification remain outside
this preview. Preserve source provenance and third-party notices; this release
does not add a license grant.
