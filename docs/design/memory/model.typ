#import "../style.typ": design-table, principle

#let fields(..rows) = design-table(
  table.header([*Field or variant*], [*Fact, authority, use and invariant*]),
  ..rows,
)

== Cell and final return <memory-cell>

Start at the point where physical storage can be given back. An allocator
can reclaim a particular allocation only with its original receipt. An
external provider can take back a grant only under the agreed conditions.
In both cases, the same physical address may later name a different
resource, and old CPU, device or software access may outlive its holder.
Each physical instance therefore needs an identity, a source that can take
it back, one movable right to dispose of it, and a way to account for every
admitted use. Together these facts define a *Cell*.

The declarations in this chapter show the facts the model must retain.
Names and containers illustrate possible implementation shapes. An
`Arc`, a `Lock` or a `BTreeMap` is a candidate only. One fact may be
encoded in an identity, index, or backend record if its enforcing owner
and lifetime remain explicit.

```rust
struct Cell { inner: CellRef }
struct CellInner { id: CellId, source: CellSource }
enum CellSource { Internal(Allocation), External(ProviderGrant) }
struct CellAccess { inner: CellRef, range: ByteRange }
```

#fields(
  [`Cell.inner`], [The unique disposition token refers to the physical
    record. Allocation or import creates the token. A holding or a
    prepared operation owns and moves it, and release consumes it.
    This link preserves exactly one way to return the exact instance.],
  [`CellInner.id`], [The Cell domain issues an identity at allocation,
    import or successful reshape. Inspection and identity checks use it.
    It stays fixed through Transfer. A fresh identity distinguishes old
    work from a later Cell at the same physical address.],
  [`CellInner.source`], [The source supplies geometry and the receipt.
    Cell fixes the source at creation. Admission reads its constraints
    and release submits the receipt to its source. Reshape replaces whole
    receipts together. A physical address cannot recover the original
    return contract.],
  [`Internal(Allocation)`], [The allocator owns the physical range,
    layout and receipt it issues. Cell reads geometry at admission and
    return, and the allocator checks any receipt conversion. The receipt
    ensures the originating allocator gets back the exact allocation.],
  [`External(ProviderGrant)`], [The provider defines the range, its
    stability, memory attributes, allowed uses and return receipt. Import
    checks the grant, admission enforces it and return honors it. These
    facts prevent use outside the granted window or lifetime.],
  [`CellAccess.inner`], [Admission binds this use to the exact Cell
    record. Its owner, such as a Mapping or table tree, retains it until
    settlement. This binding keeps the instance alive through Transfer
    and through a request to return the Cell.],
  [`CellAccess.range`], [Admission fixes a byte range relative to that
    Cell. Consumers read it to compute the physical extent and check
    coverage. Losing it would let a subrange admission authorize access
    to adjacent bytes.],
)

A `Cell` moves without cloning. A `CellAccess` can be handed to the owner
responsible for this use. It cannot create another access or recover
disposition. A descriptive ID cannot acquire either. Backends must preserve
the admitted use and its obligations, including attribute and consistency
constraints. How those constraints are carried or registered, and when
access must be exclusive, remain implementation and platform questions.

- `allocate` obtains an Internal allocation. Prepare and zero ordinary
  newly allocated Internal RAM before publishing it to its users.
- `import` accepts an External grant and preserves the original return
  route. Failed import leaves the grant with its owner. The provider
  contract governs initialization, including ordinary RAM and register
  windows.
- `admit` validates the byte range, use and memory attributes against the
  source and applicable platform policy. The operation separately
  validates its live authorization. Access owners record what must settle
  before they can release an admitted use.
- `release` consumes disposition and closes the path for new admissions
  through it. After the last previously admitted use settles, the Cell
  domain returns the original receipt once. If return is asynchronous, a
  durable owner keeps the receipt until the source has accepted it.

#principle[
  Physical reuse requires both returned disposition and settled access.
  A reference may preserve storage. Hardware completion must be proved by
  the owner that admitted and retained the hardware effect.
]

== Flat and stable logical holding <memory-flat>

A loader or pager needs a logical page position that survives changes in
physical backing. The Cell's physical address cannot supply that coordinate.
A *Flat* fixes its coordinates when created. A *Holding* occupies a
logical interval and owns the unique Cell disposition there. The start is
the index key. The extent follows from the Cell's geometry in base-page
units. Different Cell sizes can coexist, and gaps require no entries.
Each held interval lies wholly inside the Flat and is disjoint from its
neighbors. CPU and IOMMU leaf sizes are decisions of their respective MMUs.

```rust
struct Flat { id: FlatId, pages: PageCount, holdings: HoldingIndex }
struct Holding { id: HoldingId, cell: Cell }
struct HeldAt { flat: FlatId, start: PageOffset, holding: HoldingId }
struct FlatPermit {
    flat: FlatRef,
    rights: FlatRights,
    window: PageRange,
    ceiling: Access,
}
```

#fields(
  [`Flat.id`], [Flat creation issues a stable space identity.
    Inspect and conditional operations compare it to `HeldAt.flat`.
    Otherwise equal offsets in different Flats could match.],
  [`Flat.pages`], [The creator fixes the base-page extent. Operations
    read it to check bounds and overflow for windows and occupied
    intervals. An empty index cannot determine the Flat's size.],
  [`Flat.holdings`], [Flat owns the sparse index keyed by logical
    start. Inspect and source selection read it. Place, Transfer, release
    and reshape update it under the invariants above. Discarding this
    relation loses both ownership at each coordinate and overlap checks.],
  [`Holding.id`], [Flat issues a new identity for each placement.
    Conditional operations compare it during commit. X can leave and
    return to the same point with the same `CellId`. Its new `HoldingId`
    prevents stale operations from acting on the new placement.],
  [`Holding.cell`], [The live entry owns the disposition token. Queries
    read its geometry and admissions consult the resource. Transfer moves
    it and release consumes it. A physical description cannot take its
    place without losing the return responsibility.],
  [`HeldAt.flat`], [Flat stamps the space when forming a descriptor.
    Selection and mutation check it against the authorized target, so an
    equal start in another Flat cannot impersonate this placement.],
  [`HeldAt.start`], [Flat reports the start of the whole held interval.
    The conditional operation uses this index key even if inspect began
    inside the interval. This preserves the location being compared.],
  [`HeldAt.holding`], [Flat reports the placement identity as observed.
    Commit compares it against the live entry. This pins the request to
    that placement even if the coordinate is reused.],
  [`FlatPermit.flat`], [The Flat domain issues authority scoped to that
    object. Pend carries it and the call resolves it. Scope must identify
    the actual resource so rights cannot migrate to another Flat.],
  [`FlatPermit.rights`], [The domain issues and attenuates operation
    rights and checks them at admission. Separate rights let a reader
    inspect or map without being able to move or dispose of the Cell.],
  [`FlatPermit.window`], [The domain fixes the authorized coordinate
    interval on issue or derivation. Access selection checks its subrange.
    Whole-Cell changes check the entire extent. Without the window a
    capability could operate outside its assigned slice.],
  [`FlatPermit.ceiling`], [The domain sets and may attenuate the
    maximum access allowed through the permit. Map and derivation read
    it at admission. Operation rights alone cannot bound mapping
    permissions.],
)

Pend owns revocation and permit ancestry. `HeldAt` describes a placement.
Source selection resolves the live permit, checks the expected placement,
converts the Cell's byte interval into Flat coordinates and prepares
a `CellAccess` under the requested use and ceiling. The Map operation
still has to commit joint source and target admission. `inspect` can
select the containing interval and report it within the caller's
authority. `place` checks the entire destination for alignment, bounds
and vacancy. Failure returns the unconsumed Cell to the responsible owner.
An empty logical position stays empty until an explicit operation
supplies backing. The index must support reserved capacity or an
equivalent failure-safe commit. An ordered map alone provides no such
guarantee.

=== Transfer, release and reshape

*Transfer.* Prepare destination index capacity, a new HoldingId and
required result space. At the common commit, recheck source `HeldAt`,
both live permits, whole intervals, exact destination vacancy, and any
restrictions that must survive the handoff. Move the same token from one
entry to the other. A same-Flat transfer uses one storage scope, and an
overlapping destination is rejected before mutation. Existing mappings
and admitted uses keep X when X moves, even if Y is placed at its old
coordinate. This operation changes holding alone. It needs neither
exclusive physical access nor any participation from the MMU. A staged
implementation needs one recoverable owner of intermediate disposition
and an unambiguous visible commit.

*Release.* Remove the exact holding and return disposition to the Cell
lifecycle. That closes new admissions through this holding. Physical
return can lag behind while prior uses settle. Closing the Flat also
closes its own admission and settles its remaining work. It has no claim
on a Cell it previously transferred away.

*Split and Merge.* Validate an interior base-page cut or logically
adjacent, physically contiguous inputs in the same order. Check the
allocator or provider can convert all original receipts into independently
valid output receipts with compatible constraints. Before consuming an
input, close its new admissions, prove all its accesses and hardware
effects settled, and reserve identities and bounded space for outputs,
index changes and recovery. Commit the receipt conversion and index
replacement together, issuing new CellIds and HoldingIds. Busy or a
failed prepare leaves every input in place with its old identities and
receipts. Provider support is conditional. Changing a hardware leaf size
does not reshape a Cell.

== Nest and address access <memory-nest>

Programs access virtual addresses and devices issue I/O addresses.
Both need address ranges that can exist before they have backing and
remain occupied while an old translation retires. A *Nest* is one
address space in one translation domain. Its address reservations and
installed uses are separate facts.

#design-table(
  table.header([*Classification*], [*What Internal and External specify*]),
  [Cell source], [Internal returns to an allocator. External returns to
    a provider under its grant.],
  [Nest domain], [Internal translates CPU virtual addresses. External
    translates device I/O addresses through an IOMMU.],
)

These are independent dimensions. An allocator-backed RAM buffer can be
mapped through both CPU and I/O spaces when the attributes and participants
are supported. An External Cell can be mapped in a CPU Nest within its
provider and platform contract.

```rust
enum Nest {
    Internal(AddressSpace<Internal>),
    External(AddressSpace<External>),
}

struct AddressSpace<M: Mmu> {
    id: NestId,
    mmu: M,
    ranges: AddressRanges<M::Address>,
    mappings: RangeMap<M::Address, Mapping<M>>,
}
struct Mapping<M: Mmu> {
    id: MappingId,
    access: CellAccess,
    rights: M::Rights,
    translation: M::Translation,
    state: MappingState,
}
enum MappingState { Installing, Active, Removing }
```

#fields(
  [`Nest::Internal`], [Nest creation chooses a CPU address space.
    The scheduler and CPU backend activate and synchronize it. The
    variant keeps CPU addressing and completion rules attached to it.],
  [`Nest::External`], [Nest creation chooses an I/O address space.
    Binding and device-translation operations use its IOMMU context.
    The variant preserves that domain's address and completion rules.],
  [`AddressSpace.id`], [Nest creation issues the identity. Permits and
    exact requests use it to distinguish spaces and later instances,
    even where their address ranges and hardware labels overlap.],
  [`AddressSpace.mmu`], [The space owns its concrete translation
    context. The backend mutates tables and advances effects, and
    activation or attachment uses this context. It binds each hardware
    obligation to the root and domain that can settle it.],
  [`AddressSpace.ranges`], [Nest owns occupancy by reservation identity,
    fixed origin, live subranges, and required route or generation checks.
    Reserve and Release change occupancy. Map and fault resolution
    verify it. Occupancy has to survive absent backing and retirement.],
  [`AddressSpace.mappings`], [Nest indexes each mapping by target
    interval, whose length must equal its CellAccess interval. Map
    inserts it, Protect and Unmap locate it, and retirement removes it.
    A record remains discoverable through partial installation and
    retirement, so overlapping operations cannot bypass the obligation.],
  [`Mapping.id`], [Nest issues a fresh ID at each new mapping,
    including the same address and Cell. Protect, Unmap, sampling and
    completion match the exact instance. Old requests and receipts
    cannot act on the replacement.],
  [`Mapping.access`], [Map admission supplies the unique use of a
    specific Cell byte range. The mapping retains it through Protect
    and Unmap until retirement. This keeps X accessible after its
    holding has moved and keeps the mapped extent allocated.],
  [`Mapping.rights`], [Nest owns the authorized permissions,
    checked against source, target and domain at Map or Protect. The
    backend enforces them and inspection reports the applicable phase.
    Losing this fact would obscure the authorized permissions and the
    old permissions that still require synchronization.],
  [`Mapping.translation`], [The backend prepares and advances the
    concrete effect record for this mapping, including partial updates,
    pending evidence and retained table pages. Install, Protect and
    Unmap use it to settle the exact hardware effect they created.],
  [`Mapping.state`], [Nest owns the local phase and moves it on
    admission, backend readiness and retirement. Calls and observers
    use it to distinguish when the full mapping is usable and when its
    address and access still need protection.],
)

#fields(
  [`Installing`], [Nest enters it when it records the admitted use
    before hardware visibility. Backend progress either establishes the
    whole mapping or leads to retirement. The phase preserves a place
    to recover partial effects without reporting the address ready.],
  [`Active`], [Nest publishes readiness after the backend establishes
    the whole supported mapping. Protect and Unmap can operate on
    it. Observers need this milestone to begin using the address.],
  [`Removing`], [Nest enters it on accepted withdrawal or cancellation
    with possible hardware effects. The backend retains pending or
    blocked work here. The phase keeps the old interval and access
    until matched evidence allows removal and eventual address reuse.],
)

=== Address operations and fault routes

#design-table(
  table.header([*Operation*], [*Commit and completion*]),
  [Reserve], [Check the Nest permit and availability. Fix a reservation
    instance, origin and live interval. It can exist without a Mapping.],
  [Map], [Check the live source (normally a Flat permit and expected
    HeldAt) and target reservation, extent, alignment, overflow, rights
    and attributes. Record admission, MappingId, exact CellAccess,
    needed authorization links and recovery space before hardware
    visibility. Report ready only after the whole installation is Active.],
  [Protect], [A reduction requires synchronization covering old rights.
    An implementation can update in place or withdraw and install a
    narrower successor while keeping occupancy. A replacement may
    move an existing access only after the old hardware use settles.
    Expansion requires new live source qualification for the same
    Cell and byte range, target permission and platform checks.],
  [Unmap], [Start withdrawal by exact MappingId. Keep the mapping,
    interval and CellAccess while effects may remain. An accepted
    request and a completed retirement are separate observations.],
  [Release], [Check the reservation, generation and live window.
    Close admission for the affected range, settle or explicitly cancel
    its outstanding mapping and fault work, or return Busy. Free only
    the settled range. A new reservation gets a fresh identity.],
)

Fault routing belongs to the reservation. A route identifies the intended
handler and generation and preserves the authorized interval and access
limits while backing is absent. A fault ticket binds the reservation and
route generation, address and access, original request and initial
deadline. A retry preserves that deadline. Resolve rechecks the ticket
and live permissions and restores execution only after usable translation.
A released interval or replaced route invalidates an old refill. Nest
owns the address and fault facts. Room owns the wait and execution
continuation.

== MMU contexts and completion <memory-mmu>

The MMU gives a Nest's address relations effect in hardware. A narrow
backend interface can share preparation and completion vocabulary while
each backend owns its actual protocol.

```rust
struct MapSpec<A, R> {
    start: A,
    physical: PhysicalRange,
    rights: R,
    memory_type: MemoryType,
}

trait Mmu {
    type Address;
    type Rights;
    type Translation;

    fn prepare(&self, spec: MapSpec<Self::Address, Self::Rights>)
        -> Result<Self::Translation, MapError>;
    fn install(&self, translation: &mut Self::Translation)
        -> Poll<Result<(), MapError>>;
    fn remove(&self, translation: &mut Self::Translation) -> Removal;
}
enum Removal { Pending, Blocked(HardwareError), Complete }
```

#fields(
  [`MapSpec.start`], [Nest supplies the checked target start.
    The backend uses it for leaf placement and synchronization coverage.
    It ties the prepared work to the authorized address.],
  [`MapSpec.physical`], [The kernel derives the extent from CellAccess.
    Preparation uses it to select supported geometry and leaves. It keeps
    the target length and physical extent within the admitted use.],
  [`MapSpec.rights`], [Nest supplies the authorized candidate rights.
    The backend checks their representability and encodes the translation.
    It must know which accesses can become visible.],
  [`MapSpec.memory_type`], [Admission derives attributes from the
    source contract and verified platform support. The backend validates
    and encodes them. Permission bits alone cannot encode cacheability,
    ordering or device memory requirements.],
)

`Address` and `Rights` express what a domain can translate and enforce.
`Translation` is the backend's durable account of a concrete installation,
edits, withdrawal, and evidence. Its internal state belongs to that backend.
`prepare` has no hardware effect and can fail without retirement.
`install` succeeds when the full translation is usable. It can make partial
changes, so an error after the first possible effect uses the same
`Translation` to withdraw them. `remove` is restartable and advances
that record within a work bound. Pending means more work or evidence is
due. Blocked preserves the exact unresolved obligation and error.
Complete means the backend has sufficient evidence for this retirement.
An interrupt or a wakeup only prompts the backend to check.

=== Internal CPU MMU

```rust
struct Internal { tables: InternalPageTable, residency: HartResidency }
struct HartResidency { revision: Revision, harts: HartSet }
```

#fields(
  [`Internal.tables`], [The CPU backend holds the CPU root and table
    tree. Activate reads the root. Map, Protect and Unmap edit the tree.
    It preserves ownership of the pages the CPU can walk.],
  [`Internal.residency`], [The CPU backend coordinates activation
    and table changes through the facts about harts and translation
    revisions. This relation keeps a switching hart from escaping the
    retirement barrier.],
  [`HartResidency.revision`], [The backend advances the relevant
    revision on edits requiring synchronization. Activation and retirement
    compare the observed revision and matching acknowledgments. This
    distinguishes a fence that covered today's edit from an older one.],
  [`HartResidency.harts`], [The backend updates the set of harts that
    may retain translations or still walk the old tree. Invalidation
    determines its required coverage from this fact. A hart that switched
    away can still owe synchronization.],
)

The scheduler calls `activate` with the authorized CPU context.
Activation selects the correct root and ASID context, including the
relevant generation where labels are reused. It is ordered with edits:
a hart that could retain old state is covered by the change, and a later
entrant observes the required revision and synchronizes before using it.
On RISC-V, `SFENCE.VMA` acts on the executing hart. Remote harts need
ordered publication of page-table writes, synchronization of the correct
ASID and address scope, and a matching acknowledgment or equivalent
trusted proof. Protection changes, non-leaf edits, old roots and label reuse
can require different scopes. Where data or instruction visibility is part
of the operation, the platform must also provide that evidence.

=== External IOMMU and requester bindings

```rust
struct External { tables: ExternalPageTable, bindings: RequesterBindings }
struct RequesterBinding {
    id: BindingId,
    requester: RequesterId,
    controller: IoControllerRef,
    phase: BindingState,
}
enum BindingState { Attaching, Active, Detaching }

struct IoController {
    directory: DeviceDirectory,
    commands: CommandQueue,
    completions: CommandCompletions,
}
```

#fields(
  [`External.tables`], [The I/O backend owns the domain root and
    table tree. Attach uses the root and Map, Protect where supported,
    and Unmap edit leaves under the domain rules. The root ties those
    effects to this I/O address space.],
  [`External.bindings`], [The I/O context accounts for its bindings.
    Attach and Detach update membership. Retirement reads current and
    still-retiring bindings to cover all requesters that could retain an
    old translation. The physical controller enforces requester conflicts.],
  [`RequesterBinding.id`], [Attachment creates a distinct binding
    instance. Detach and hardware acknowledgments compare it. Reattaching
    the same requester cannot consume an old detach receipt.],
  [`RequesterBinding.requester`], [The trusted platform resolves the
    isolatable request source. Attach uses it for authorization and
    directory programming, and Detach and fences use it to scope work.
    The requester identity establishes which traffic may traverse the root.],
  [`RequesterBinding.controller`], [The platform resolves the physical
    controller for that source. The binding operation fixes the
    relationship; command submission and retirement follow it. Matching
    a requester or receipt across the wrong controller would settle the
    wrong traffic. This relationship can be encoded in requester identity.],
  [`RequesterBinding.phase`], [The I/O backend advances the binding
    from Attaching, through Active, to Detaching. Submission and
    reattachment use these states to distinguish admitted traffic from
    incomplete setup or retirement.],
  [`IoController.directory`], [The physical controller backend updates
    requester-to-context entries at attachment and detachment. It owns
    conflict checks across Nests using this controller. Without that
    shared scope two contexts could publish the same requester entry.],
  [`IoController.commands`], [The controller backend owns queue
    capacity and sequences applicable invalidations and fences across
    contexts. Ordered submission and tracked capacity bind effects to the
    commands actually accepted.],
  [`IoController.completions`], [The controller backend matches real
    hardware results to command and operation identities. Translation and
    binding retirement consume only their matching coverage, including
    when several contexts were batched. A queue notification alone
    cannot identify which obligation settled.],
)

`begin_attach` enters Attaching and prepares the requester context.
The backend publishes it and synchronizes it with the current
translation revision before Active can admit device traffic.
`begin_detach` names the exact binding and closes new admission. The
backend retains the requester, root and binding generation until all
relevant obligations settle. Attach needs authority over both the
requester and its destination. Controllers can be shared by distinct
External Nests while directories, queues and sequence spaces remain
controller-owned. The supported isolation granularity, sharing topology
and exact binding policy require platform qualification.

Removal covers the translations, binding epochs and controllers that
could use the old mapping, including detach already in flight. Obtain
the applicable directory, IOMMU translation and device-cache
invalidations, required ordering, and trusted evidence for outstanding
device transactions and table walks. A RISC-V `IOFENCE.C` has defined
scope over commands and already processed I/O requests. Transactions
still upstream can need separate platform drain or device quiescence.
Device command completion may license use of the data under the device
protocol, while translation retirement still owes these proofs.
Support for ATS, PRI, bypass, recoverable device faults or noncoherent
sharing requires a separate qualified contract for the actual platform.

The architectural sources give the scope of translation fences and
I/O command completion.
#link("https://docs.riscv.org/reference/isa/priv/supervisor.html")[RISC-V Supervisor]
· #link("https://docs.riscv.org/reference/iommu/v20260222/iommu_in_memory_queues.html")[RISC-V IOMMU command queues]

== Page-table ownership and pruning <memory-tables>

A translation root leads to a tree of tables owned by its backend.
A branch PTE points to a child table, and a leaf PTE describes the data
access retained by its Mapping. The same ownership shape works with
different CPU and I/O page sizes, geometries and entry formats.

```rust
struct PageTable<F: TableFormat> { format: F, root: TableNode }
struct TableNode { page: TablePage, children: ChildIndex }
struct TablePage { storage: CellAccess }
struct TableEffect<F: TableFormat> {
    invalidation: F::Invalidation,
    detached: DetachedTables,
}
```

#fields(
  [`PageTable.format`], [The backend fixes supported levels, entry
    rules, alignment and leaf sizes for this context. Lookup, planning
    and commit use them. Invalid geometry could make hardware
    interpret a different range or entry kind.],
  [`PageTable.root`], [The translation context owns the root node.
    CPU activation or requester attachment publishes its address, and
    tree edits start here. Root ownership retains all reachable table
    storage until the context proves old walks cannot continue.],
  [`TableNode.page`], [The owning node retains the actual table page.
    The backend initializes and edits this page and hardware reads it.
    The link keeps software ownership attached to the corresponding PTEs.],
  [`TableNode.children`], [The tree owns linked subtrees by parent
    entry index. Lookup and planning read the relation. Commit attaches
    or detaches these nodes with the branch PTE. A leaf owns no child.
    The correspondence prevents double ownership and premature return.],
  [`TablePage.storage`], [The backend retains the table's dedicated
    CellAccess, established before publication. It uses the access to
    locate the table and keeps it through late walks. Retire ends it
    after proof. An address alone cannot retain the storage.],
  [`TableEffect.invalidation`], [Commit records the actual edits and
    scope required by the backend, including non-leaf changes. Invalidate
    uses it to identify the needed coverage. Leaf-only evidence cannot
    discharge an unrelated branch edit.],
  [`TableEffect.detached`], [Commit hands detached subtrees to the
    corresponding translation or context retirement record. Retire
    releases them after their own evidence. This preserves reachability
    from software while stale hardware walks may still reach them.],
)

A runtime table page comes from a suitably sized and aligned Internal
Cell. In the shape above, creation admits a dedicated table use and
returns temporary disposition to the Cell lifecycle. The dedicated
access prevents physical return until the table retires. The same Cell
reclamation conditions apply as for mapped data.

+ *Plan* the edit from the current tree path, expected entries,
  required table pages, and retirement capacity. Root creation,
  root-address lookup and leaf lookup are local backend operations.
+ *Reserve* new pages, index storage and detached-page records outside
  the tree write scope. An allocation failure here leaves it untouched.
+ *Commit* under tree protection after revalidation. A path conflict
  retries without effects from that step. Initialize a new child before
  linking its parent, follow the hardware entry-update and A/D rules,
  and record each visible effect. If the change needs staged edits,
  including a large-leaf replacement, keep its address and recovery
  record across steps.
+ *Invalidate* and check the backend's completion evidence for the actual
  edits. Wait outside the tree and address critical sections.
+ *Retire* detached tables once evidence covers old table walks.
  Retire data access and table storage according to their respective
  obligations, even when one backend batch contributes evidence to both.

If neighboring leaves A and B share a last-level table T, removing A
keeps T. Removing B can clear its leaf, confirm T is empty and unlink the
parent branch under the same tree protection. The detached T remains in
the retirement record until old non-leaf paths and walks are covered.
Prune back along the modified path when ancestors also become empty.
The single-path work space is bounded by tree height and larger ranges
are processed in bounded batches. An empty Nest can retain its root.
Destroying it requires switching CPUs away or detaching requesters as
appropriate, then proving old roots, labels and walks safe to retire.
