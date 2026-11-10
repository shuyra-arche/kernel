= Isolation and the problem of composition

== What is being isolated?

Within one execution domain, computation can exchange values through memory
and coordinate progress through control state. Once a system separates its
programs into protected domains, direct access no longer carries across those
boundaries. This is intentional: a domain must not acquire another domain's
memory or execution control merely by holding a pointer or identifier.

We use *Unit* to describe a protected subject in the architectural argument.
This name does not prescribe a separate runtime container for every type of
object in the implementation. The current Kernel exposes Teams as protection
and sharing domains, and Tasks as execution instances within them.

== The central question

Isolation without interaction is insufficient for a useful operating system.
Interaction without enforced boundaries would undo the reason for isolation.
The Kernel therefore enables protected domains to compose their data and
control flows, while retaining the constraints that made them independent.

For any crossing, the design must answer:

- *What* crosses: bytes, a bounded right to access existing storage, an event,
  or authority to control an execution instance?
- *Who* authorizes the crossing, and can that authority be narrowed or revoked?
- *Which identity* does the crossing refer to, including after resource reuse?
- *When* is the transition committed and visible to its participants?
- *What survives* cancellation, failure, exit, or an unfinished hardware action?

The Kernel's purpose is not to make isolation invisible. It is to make
composition explicit enough that safe behavior can be reasoned about.

== Scope and non-goals

The Kernel owns the mechanisms needed for execution, isolation, access
authorization, and safe resource retirement. Device protocols, PCI
enumeration, service placement, page-replacement policy, and language-runtime
facilities are responsibilities of other providers or of system composition.

A source/build Cluster is an organizational unit, not a Kernel protection
domain. Conversely, the internal names of Kernel mechanisms are not, by
themselves, the public model of the operating system.

The following chapters explain the flows and safety conditions first.
They intentionally do not follow the `basis`, `entry`, and `metal` source
directories chapter by chapter. Those directories are mapped separately in
the documentation's source-navigation index.
