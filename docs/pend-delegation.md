# Cross-Task Pend delegation and registration admission

`Pend.Accord` derives the caller's current live authority into a live receiving
Task. Unit creation topology does not authorize resource delegation. The caller
does not gain execution control, access to the recipient's other grants or a
new resource owner identity. Unit's own management/administration checks retain
their separate topology and control rules. Mark remains descriptive metadata.

## Fixed registration contract

The initial policy uses existing Pend/Ledger boundaries, with no wire-layout,
ABI version or additional top-level authority object:

| Budget | Bound and owner |
| --- | --- |
| Recipient ledger | 1,024 total registrations, including unpublished preparation. |
| Incoming delegation | At most 256 registrations whose actual sender is another Task. The other 768 slots remain available to local operations even under incoming saturation. Local operations may use all 1,024 when incoming usage is lower. |
| One sending Task → one recipient | At most 32 outstanding incoming registrations, across all source Pends. |
| One sending Team → one recipient | At most 128 outstanding incoming registrations, across all its Tasks. |
| Sender's outgoing/derived edges | Existing Heirs limit of 1,024, including prepared edges. It bounds unsettled delegation obligations across recipients and parents. |

The receiving Table's Admission owns registration charges. Preparation reserves
the total and incoming/sender/Team budgets before publishing anything. The guard
rolls back on allocation, authorization or effect-preflight failure. Publication
moves the charge into the single PendEntry; Return/Revoke/exit release it only
after that entry's required cleanup and settlement. A failed hardware withdrawal
continues to consume its registration budget. No timeout, cancellation request
or dead sender releases an unsettled charge.

Sender TaskId and TeamId are immutable accounting metadata taken from the actual
parent Task, retained even after its Weak reference expires. They grant no
authority and do not replace the live Pend ancestry. A sender cannot reset its
per-Task budget by using another parent Pend; several Tasks in one Team share
the per-Team budget. Task/Team IDs do not wrap or reuse within this contract.

This is a bounded default receive policy, installed when the Task's Table is
created. Dormant Tasks receive under the same policy without executing a Pull or
sending a message first. Capacity failure returns the existing
`PendFail::OutOfMemory`; ledger/effect contention uses Busy. Invalid source,
scope, operations or redistribution uses Denied; existing dead-identity semantics
are preserved. Preparation failure publishes neither an heir nor a registration.

Multiple independent Teams can still saturate the 256 incoming slots; this
policy promises resource isolation and bounded abuse, not fair service for every
sender. The recipient can enumerate and Return unwanted grants without granting
their sender access to other entries. Source Revoke/Return, recipient exit and
normal lineage cleanup reclaim settled budgets. Local work retains its reserved
capacity during saturation. No incoming grant evicts or overwrites another.

## Authorization and effects

| Step | Owner, locks and failure responsibility |
| --- | --- |
| Admission / derive | Ordered source/target ledger serials, then COMMIT. Recheck both Tasks, source identity, live ancestor authorization, distribution, permission/data ceilings, normalized scope and creation license. |
| Prepare | PendEntry owns one derived live ancestry; source Heirs reserves its edge; target Registration reserves its exact ledger slot and Admission charge. Failure drops unpublished guards. |
| Ordinary Share | SharingOperation serializes source windows and map ownership. The child remains within source access and range limits; publishing hardware is a separate operation. |
| Borrowed device registers | The actual source authority must permit lending. Source IRQ withdrawal and mapping retirement run outside COMMIT while both ledger serials and SharingOperation retain the transaction. COMMIT is reacquired; Tasks, live ancestry/limits/license and source publish admission are rechecked before lending. Already issued hardware responsibility stays with its original Nest/Memo owners on failure. |
| IRQ | Source.attach requires the child's live LISTEN/CONSUME authority, no redistribution, and the recipient's existing matching whole writable register View. The source's own state preflights attachment and refuses an existing binding. A TaskId alone cannot reroute or overwrite a source. IRQ notification is not hardware retirement proof. |
| Publish | Under COMMIT, effect preflight completes before the infallible heir and registration publications. Both ledger serials remain held; no observer sees an orphan half-commit. Existing Sign notification runs after publication. |

The same path supports unrelated U→S reply grants and S→Dormant U initial
capabilities. Twine continues to check actual Kernel-provided requester/replier
identity. Provider changes land first; consumers pin the verified immutable
Kernel revision before replacing older adjacency-rejection integration tests.
Host ledger tests exercise production authority/admission and scripted effects;
RV64/QEMU and consumer tests establish their separately recorded boundaries.
