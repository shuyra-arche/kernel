# Ordinary Task startup (ABI 14)

`UnitCreate::Task { team, start: TaskStart }` creates a dormant User Task from
memory already prepared in its target Team's Nest. It never copies argument
words from the creator or allocates, selects, or owns the user's stack.

The 48-byte snapshot is six little-endian RV64 words:
`[2, team-or-zero, entry, stack_pointer, arguments, argument_count]`.
Tag 1 (ABI 11/12 Kernel-allocated stacks) is rejected. ABI calls, Ledger and
Capsule's required ABI byte are 14; Capsule format remains 2. Rebuild all
consumers against one exact provider revision. ABI 12's User ECALL transport
and Kernel-only Supervisor boundary remain in force.

The Kernel initializes PC, SP, a0=arguments and a1=argument_count, zeros every
other user GPR, and supplies privilege, address space and private trap state.
The creator cannot inject SSTATUS, SATP, Kernel SP or supervisor execution.

Entry must be nonzero, 2-byte aligned and executable in the target Nest. SP must
be nonzero, 16-byte aligned and at most the user ceiling. It is an exclusive
bound: the byte at SP-1 must be currently writable, while SP itself may equal
the span end or user ceiling. No stack address or size is inferred.
Arguments contain at most 64 aligned 8-byte words; nonempty arguments must be
non-null and their entire checked range readable in the target Nest. Empty
arguments may use zero. Cross-page ranges are validated page by page.

Only the initial stack access must be resident. The rest of a runtime's stack
may use existing Memo materialization/fault mechanisms; this version does not
admit an absent initial stack byte solely from a FaultRoute. Guards are ordinary
unmapped span positions, never a new permission class.

Creation prepares a private Kernel execution frame, then rechecks the live
creator, target Team management identity, User mode and mappings under COMMIT
before publication. A Team operation lease excludes concurrent Team retirement.
Failure rolls back private execution storage; user memory stays with its actual
owners. No caller-supplied physical pointer is dereferenced.

Mapping checks describe the creation commit, not a permanent mapping lease.
After publication, an authorized memory owner may retract or change a mapping
before Resume. The Task then follows ordinary instruction/store/load fault
handling on first use, including current fault routing and invalidation; it does
not gain a stale copy or ownership of that mapping. Changing argument contents
remains an ordinary shared-memory synchronization responsibility.

Task exit/reclamation concerns execution storage. Runtime retains stack Flat
and Span grants, waits for Task reclamation, and separately settles Span
deletion/retirement before deleting backing or reusing addresses. Exit or
Join alone is never physical-retirement evidence. Failed or pending memory
operations retain their original request identity and resources.

The private `creation::Bootstrap` prepares the initial User Capsule transition
stack and Kernel resident-work stacks. It is not reachable through Unit.Create.
All external Capsules and ordinary Tasks execute in User mode. Hart trap stacks
and Kernel observer work remain Kernel-owned.

Consumer migration order is Kernel, Toolchain Broker/threads and System Loader,
then Vanllia composition. Toolchain must preserve explicit stack bounds for TLS
and backtraces; System must publish stack/arguments into the target Nest before
creating its Task. Each owner must retain partial preparation and clean up only
after its real memory operation settles. Kernel-only tests do not constitute
assembled System/Toolchain acceptance.

Optional real-time admission retains its existing restricted-residency policy:
it resolves the current Span containing SP-1, with a 64 KiB maximum,
before admission. Runtime must prepare that body as fully writable/resident and
keep any absent guard in a separate Span. This is an RT admission
constraint, not ordinary Task creation policy or Kernel ownership of the stack;
subsequent authorized memory withdrawal follows the existing RT fault behavior.
