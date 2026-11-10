= Reconnecting data and control flows

== Data: independent delivery or bounded access

A data flow across protection boundaries can take two fundamentally different
forms. A *copy* creates independent content whose subsequent use does not
depend on the original storage's continuing access grant. A *borrow* provides
access to existing content, and therefore retains a relationship to its owner
and to the access restrictions on that content.

The current public model names these interactions `View.Clone` and
`View.Share`. These are semantic choices, not two styles of page-table
mapping. A shared view does not create another physical owner, and a copy
does not retrospectively grant access to the sender's original storage.

A logical reference to data, its physical backing, and the translation that
makes it accessible are distinct relationships. Implementation mechanisms
may retain them for different periods; none may silently turn an old logical
reference into authority over newly allocated physical content.

== Control: events, execution, and results

A control flow may ask another subject to perform work, observe a state
change, await completion, or receive an event. A notification does not,
by itself, give the recipient permission to control the source subject.

The current model uses `Sign` for retained event observation and routing.
Execution and its results remain the responsibility of the relevant Unit
lifecycle and scheduling mechanisms. An event saying that a Task finished
cannot substitute for the Task's authoritative result, and consuming an
event cannot by itself cancel work or discharge hardware responsibilities.

== Authority connects the two flows

Every interaction has a permission boundary separate from the identity of
the target. A visible identifier is not a credential.

`Pend` is the current Kernel's public representation of scoped operation
authority. It distinguishes target kinds and permitted operations, and,
where relevant, the range, mutability, and distributability of a borrowed
right. `None` represents explicit permission to perform specified
object-free operations, not ambient access to all existing objects.
Execution-control authority is distinct from data authorization.

The general composition pattern is:

+ Identify the source, destination, and exact operation.
+ Establish an authorized path and its effective scope.
+ Decide and commit a valid state transition under its enforcing owner.
+ Perform any necessary data, scheduling, or hardware effects.
+ Retain enough state to observe completion, failure, and final cleanup.

These steps explain *responsibilities*, not a required syscall ordering.
They may be combined within one atomic operation or separated into
asynchronous stages, provided their invariants remain enforced.

== Why the components cannot be collapsed

A data handle cannot also be presumed to own execution-control rights.
A notification cannot also be treated as a memory fence. A physical reference
cannot also imply that sharing is authorized. Conversely, separating every
internal operation into a new public object would burden the model without
necessarily protecting another invariant.

The correct boundary is the place that owns a fact, enforces its transitions,
and remains accountable for partial failure. Source-code names are helpful
for locating that owner, but do not define the theory of composition.
