# IOMMU partial progress and resource ownership

The coherent QEMU virt adapter supports its admitted immutable requester route,
Sv39 table window and disabled bypass/ATS/PRI/PQ configuration. Unsupported
hardware is refused rather than assigned a software completion. Each exclusive
Adapter operation owns its progress step; Nest owns payload Publications, Domain
owns the four table Retentions, and Engine owns three queue/completion Retentions.

| Boundary | Possible hardware effects | Retained owner and next step | Reuse evidence |
| --- | --- | --- | --- |
| Validate / prepare | None; DDTP remains Off. | Unpublished Retentions cancel on failure; controller remains available. | No publication occurred. |
| First publish / Starting | Tables published; directory purge can be queued before DDTP installation. | Leaf Starting and Domain retain context/tables; Nest's Publishing/Failed Publication retains the payload. Closing may advance normal incomplete activation, but a failed Engine is quarantined. | Activation never proves payload retirement. |
| Later publish / Unconfirmed | New leaf PTE may be visible before command submission succeeds. | Adapter retains an Unconfirmed Leaf even if no batch was returned. Ready refuses it and capacity closes. Withdrawal may clear the leaf and request its own fresh fence if Engine is healthy; hardware faults fail closed. | No readiness inherited from the active context or another leaf. |
| Engine submit before queue write | Validation/health/DDTP/batch allocation can fail. An already written Domain leaf is still possible. | Engine retains queues; Leaf/Domain retain PTE responsibility. State/identity errors must not imply rollback of a written leaf. | A separate exact withdrawal is required after a leaf write. |
| Queue memory written / CQ Tail written or lost | Commands and completion reset exist; CQ Tail may have reached hardware partially or made no progress. | Engine records Waiting(exact batch) before MMIO Tail. One outstanding batch prevents resubmission or stop. Observation detects queue faults or remains pending; Drop quarantines published queues. | Tagged completion **and** matching CQH/tail, healthy CQ/FQ/PQ/DDTP, ordering and exact batch. Waiting itself is not proof. |
| Leaf withdrawal | PTE cleared before invalidation submission. | Requested/Flushing retains exact projection and batch; failures leave payload/table/queue responsibility. A platform retry preserves a live/completed hardware fence, using a fresh outer attempt. | Exact withdrawal CQ fence followed by an independently matching range Quiescence receipt. |
| Late/wrong/incomplete completion | IRQ, head advance, stale word, partial fence, fault record or unrelated batch may be observed. | They cannot settle the Publication. Wrong batch does not replace Waiting; CQ/FQ/PQ errors make Engine Failed. Removing error bits cannot revive Failed. | Match controller, requester, Binding epoch, attempt and complete range. |
| Shutdown / Closing | Leaf teardown, context revoke, DDTP Off and directory invalidation progress in separate stages. | All leaves must settle before context closure. Closing retains Domain/Setup and a distinct Binding ticket while platform proof is pending or wrong. | Closed Domain plus exact Binding Quiescence receipt; faults are drained before Engine is returned. |
| Engine stop | CQ then FQ disabled asynchronously, DDTP Off, PQ disabled. | Published queues remain until both queues are off/not busy and the last command/completion is covered. Failed/Waiting cannot be stopped into Closed. | Explicit proven-off path settles the three queues. Driver/task exit and Drop cannot supply this evidence. |

The existing state model represents these responsibility boundaries. In
particular, Domain's Active phase can coexist with a failed leaf operation while
Engine and Adapter reject further publication; it does not attest quiescence.
No additional recovery ledger or unconditional error-to-Closed transition is
needed. Permanent fail-closed quarantine is intentional when trustworthy
completion cannot be recovered.

The Host harness imports production Engine, Domain, Adapter, Publication and
Page retention code. It scripts register visibility and independent platform
receipts. Regression cases interrupt submission after queue memory writes,
after CQ Tail visibility, or by losing Tail, then abandon the source and assert
that payloads, tables and queues cannot be reused. Existing tests cover pre-write
failure after a leaf PTE write, startup/queue faults, wrong/late batches,
insufficient completion, proof identity, closing during activation and separate
Binding retirement. These tests do not establish physical DMA quiescence.

The guest IOMMU suite executes the admitted QEMU hardware backend, tagged
completion, delayed/denied DMA and reuse; the External Nest multi-Hart cases also
exercise source exit and independent leaf retirement. QEMU does not establish
physical-controller behavior, noncoherent memory support or real-hardware
timing. Those remain outside the preview's supported evidence.
