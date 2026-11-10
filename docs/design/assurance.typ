= Safety and completion obligations
<assurance>

== Identity and authority

A stable name identifies an object. A right authorizes an action on that
object. Both may be represented by handles, but they answer different
questions: knowing a name is never sufficient proof of permission.

When internal slots or hardware binding positions are reused, an old
observation must not acquire authority over the replacement. The enforcing
owner must either reject the stale request or qualify it with the exact
identity, generation, and scope needed by the operation.

== A request is not its completion

An accepted request may begin an asynchronous transition. Its effects may
still be in flight after the caller exits, an event is consumed, or a
timeout is observed. The Kernel needs distinguishable states and retained
responsibility for these cases.

For resource retirement, four obligations are particularly important:

+ *Admission closes:* no new conflicting access may begin.
+ *Existing access is withdrawn:* the relevant mappings or access grants
  cease to admit new operations under the responsible mechanism.
+ *Prior effects are settled:* trusted invalidation, ordering, and completion
  evidence proves the former access is no longer capable of touching the
  protected resource.
+ *Reclamation follows:* only after all holders and hardware obligations
  have settled may the underlying capacity be reused.

A device reporting work complete is a statement about its operation, not
necessarily a proof that old translations, queued requests, or other
initiators can no longer access memory. CPU and external access need
evidence appropriate to their respective backends.

== State and effects are separate reasoning steps

For each protocol, specify the current state, incoming event, permitted
transition, and effects to perform. A useful conceptual shape is
`transition(state, event) -> decision`. The commit owner must revalidate
observations that could have changed before applying the decision.
In-flight effects require further completion or failure events.

This does not require copying the entire Kernel state or running every
implementation function as a pure function. It requires explicit ownership
of invariants and explicit handling of concurrent or failed effects.
An unobserved partial failure must not discard the record that makes
subsequent recovery possible.

== What counts as evidence?

A claim about an API representation can be checked with ABI and host
contract tests. A claim about privileged translation or interrupt behavior
requires target execution. A claim about a physical controller cannot be
established solely by emulation. Each design claim must identify the
smallest adequate verification boundary and the remaining assumptions.

External access requires an authenticated hardware requester, scoped
authority, and trusted retirement evidence. Trusted boot admits an External
Nest for the requester and supplies its root Pend. A derived Pend delegates
a subset of that authority. The hardware backend supplies the evidence
needed to settle former access before the affected memory can be reused.

The precise current operations, encoding, and supported hardware limits
are specified in the Kernel API and implementation, rather than in this
architectural argument.
