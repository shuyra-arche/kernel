= Reconnecting data and control flows
<composition>

Consider a producer and a worker in different protection domains. The
producer wants the worker to read a request and begin processing it. The
Kernel has to answer three questions independently: which bytes the worker
may read, what authority permits that access, and what event the worker
can observe. The producer and worker decide what the request means and
what counts as a successful response.

== Choose the relationship to the data

A copy gives the worker independent content. Once the copy has been
published, changes to the producer's buffer cannot change the delivered
bytes. Borrowing instead gives the worker bounded access to existing
storage. The bounds, access mode, and lifetime of that grant matter
because the underlying data remains shared.

The public model expresses these choices as `View.Clone` and `View.Share`.
A Clone delivery may itself be mapped using a read-only Share: the worker
then borrows the Kernel's copy, while remaining independent of the
producer's original storage. The two names describe relationships to data
at different points in that delivery.

== Bound the authority

A `Pend` associates a holder with a target and the operations the holder
may perform. A data grant can also carry a window and a `Modifiable`
limit. `Distributable` governs whether the holder can derive another
grant. A worker can therefore receive permission to write without
receiving permission to delegate, or receive a read-only grant that it
can narrow further and pass on.

Derivation keeps the relationship to the parent. The child's operations
and applicable limits must fit the parent's effective authority. For a
memory window, a relative offset is interpreted inside the immediate
parent's window. A descendant cannot use an offset into the original
object to recover a portion that an intermediate grant excluded. A
derived branch remains subject to retirement through its lineage.

A `None` target expresses permission for specified calls without an
existing object, such as creating a Clone View. It still carries explicit
operation rights. Managing another Task's execution requires its own
authority, independently of any View or Sign granted to that Task.

== A copied request

Suppose the worker should process the bytes that the producer submits
and the producer should be able to reuse its buffer as soon as submission
succeeds. A Clone queue gives those two lifetimes separate storage.

+ The producer uses a Pend with push authority to submit the bytes. The
  Kernel copies them into its own storage and commits one queue entry.
  The resulting payload is immutable.
+ The worker uses its pull authority. On success, removing the queue
  entry and publishing a read-only payload Pend happen together. If the
  worker's length limit is too small or the Pend cannot be prepared, the
  message remains queued.
+ The worker opens the payload to map the existing copy. It receives the
  exact byte length from Pull. A failed Open leaves the delivered Pend
  available to retry. Returning that Pend releases the worker's access
  through the normal retirement path.

This separates the queue's responsibility from the worker's. Before
delivery, the queue retains the payload. After delivery, the worker
can retain it even if the queue closes. A worker that exits after Pull
does not cause automatic redelivery. Processing and retry policy belong
to the communicating domains.

== A borrowed request

Suppose instead the request consists of a range of existing Cells and
the worker needs to operate on that storage directly. The producer
delegates a Share Pend for the required window. The worker opens its
branch to establish a mapping within that scope.

Readers may coexist. A writable borrower excludes other borrowers of
that Share. In the current model, a mutable parent is suspended while
any child remains, even when the child covers a narrower window. The
Kernel withdraws conflicting CPU mappings before granting the borrow.
Returning or revoking the branch restores the parent's access only
after the descendants and applicable hardware obligations have settled.
A pending withdrawal therefore delays handback even if the worker
has already reported that it finished.

The backing Page keeps its identity through the borrow. Granting access
changes who may use it and under what conditions. Mapping publishes
an address for that access. The borrow, the mapping, and the backing
allocation can each end at a different time.

== Notify the worker

A `Sign` provides retained event observation and routing under its own
Pend. The producer can hold trigger authority while the worker holds
listen or consume authority. Those grants leave the request's View
permissions and the worker's execution-control permissions independent.

If the producer explicitly triggers a Sign after submitting a request,
the queue submission and the trigger are separate transitions. A
trigger can report `Busy` because an earlier direct event is still
pending. That leaves the submitted request queued. The receiver treats
notification as a reason to inspect the queue and uses Pull to claim a
message. Consuming an event acknowledges that event sequence. Taking
a request or observing a Task's result still uses the owning operation.

The example has three separate commit points. Push publishes the queued
bytes, Pull transfers responsibility for the copy to the worker, and
Trigger publishes an event. This separation makes failures observable.
A failed trigger leaves a successful Push committed, and a successful
notification can outlive the condition that prompted it. The same
reasoning applies when a result or a hardware action has its own
completion condition. @assurance explains the evidence required before
the Kernel can release the underlying resources.

For current call permissions and failure results, see the
#link("https://github.com/shuyra-arche/kernel/blob/main/api/README.md#authority-and-control-contracts")[Kernel API].
