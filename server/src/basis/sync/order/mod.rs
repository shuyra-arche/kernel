//! Semantic identities of real lock storage, independent of the old ranks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Class {
    #[allow(
        dead_code,
        reason = "only constructed by the injected Flat guest contract probe"
    )]
    ProbeRegistry,
    LiveChanges,
    PageMembership,
    ChangeBlocks,
    RetentionStatistics,
    Commit,
    Flat,
    SourceAuthorizationRef,
    SourceMaps,
    SourceAuthorities,
    ActivityRegistry,
    ActivityDelivery,
    FaultPolicy,
    SpanExports,
    Reservation,
    Organizations,
    NestAuthority,
    FaultProgress,
    SourceIndex,
    ActivityHistory,
    Address,
    Cleanup,
    Nest,
    PendState,
    PendParent,
    LedgerAdmission,
    LedgerCharge,
    LedgerSerial,
    LedgerEntries,
    LedgerHeirs,
    BlockPages,
    BlockEpoch,
    BlockUses,
    BlockStages,
    Holds,
    HeldSources,
    SourceFlights,
    SourceGate,
    SourceBindings,
    SourceBlocks,
    SourceOrigins,
    SourcePhase,
    ProvenanceGrant,
    ProvenanceUses,
    Deadline,
    InterruptController,
    InterruptProgress,
    InterruptSources,
    InterruptSource,
    Scheduler,
    SchedulerAdmission,
    Shutdown,
    SignQueue,
    SignState,
    SignRouting,
    SignDelivery,
    WaitHolders,
    WaitSites,
    ControlTable,
    LifecycleDiagnostics,
    TerminationQueue,
    ReapQueue,
    Husks,
    TaskState,
    TaskReceipts,
    TaskResult,
    TaskJournal,
    TaskHeir,
    TaskRetirement,
    TaskSchedule,
    TaskExtensions,
    TaskMemory,
    Registry,
    TeamState,
    TeamTasks,
    TeamChildren,
    TeamTermination,
    TeamHeld,
    TeamExit,
    CloneQueue,
    BootMmio,
    BootRegions,
    BootNests,
    BlockPool,
    BlockPump,
    Tally,
    Bump,
    Frame,
    Spare,
    Controllers,
    Asid,
}
impl Class {
    #[cfg(debug_assertions)]
    pub(crate) const ALL: [Self; 91] = [
        Self::ProbeRegistry,
        Self::LiveChanges,
        Self::PageMembership,
        Self::ChangeBlocks,
        Self::RetentionStatistics,
        Self::Commit,
        Self::Flat,
        Self::SourceAuthorizationRef,
        Self::SourceMaps,
        Self::SourceAuthorities,
        Self::ActivityRegistry,
        Self::ActivityDelivery,
        Self::FaultPolicy,
        Self::SpanExports,
        Self::Reservation,
        Self::Organizations,
        Self::NestAuthority,
        Self::FaultProgress,
        Self::SourceIndex,
        Self::ActivityHistory,
        Self::Address,
        Self::Cleanup,
        Self::Nest,
        Self::PendState,
        Self::PendParent,
        Self::LedgerAdmission,
        Self::LedgerCharge,
        Self::LedgerSerial,
        Self::LedgerEntries,
        Self::LedgerHeirs,
        Self::BlockPages,
        Self::BlockEpoch,
        Self::BlockUses,
        Self::BlockStages,
        Self::Holds,
        Self::HeldSources,
        Self::SourceFlights,
        Self::SourceGate,
        Self::SourceBindings,
        Self::SourceBlocks,
        Self::SourceOrigins,
        Self::SourcePhase,
        Self::ProvenanceGrant,
        Self::ProvenanceUses,
        Self::Deadline,
        Self::InterruptController,
        Self::InterruptProgress,
        Self::InterruptSources,
        Self::InterruptSource,
        Self::Scheduler,
        Self::SchedulerAdmission,
        Self::Shutdown,
        Self::SignQueue,
        Self::SignState,
        Self::SignRouting,
        Self::SignDelivery,
        Self::WaitHolders,
        Self::WaitSites,
        Self::ControlTable,
        Self::LifecycleDiagnostics,
        Self::TerminationQueue,
        Self::ReapQueue,
        Self::Husks,
        Self::TaskState,
        Self::TaskReceipts,
        Self::TaskResult,
        Self::TaskJournal,
        Self::TaskHeir,
        Self::TaskRetirement,
        Self::TaskSchedule,
        Self::TaskExtensions,
        Self::TaskMemory,
        Self::Registry,
        Self::TeamState,
        Self::TeamTasks,
        Self::TeamChildren,
        Self::TeamTermination,
        Self::TeamHeld,
        Self::TeamExit,
        Self::CloneQueue,
        Self::BootMmio,
        Self::BootRegions,
        Self::BootNests,
        Self::BlockPool,
        Self::BlockPump,
        Self::Tally,
        Self::Bump,
        Self::Frame,
        Self::Spare,
        Self::Controllers,
        Self::Asid,
    ];
}

// Private supertraits prevent a domain from implementing a class or adding an
// ordering edge outside this one reviewed table.
mod sealed {
    pub trait Class {}
    pub trait Relation<N> {}
    pub trait Construction {}
    pub trait Root {}
    pub trait Pair {}
}
pub(crate) trait Order: sealed::Class {
    const CLASS: Class;
}
pub(crate) trait Before<N: Order>: Order + sealed::Relation<N> {}
pub(crate) trait Constructible: Order + sealed::Construction {}
pub(crate) trait RootOrder: Order + sealed::Root {}
pub(crate) trait PairOrder: Order + sealed::Pair {}
macro_rules! classes {
    ($($name:ident),* $(,)?) => {$ (
        #[allow(dead_code,reason="semantic categories are audited before each owner migrates to typed acquisition")]
        pub(crate) enum $name {}
        impl sealed::Class for $name {}
        impl Order for $name { const CLASS:Class=Class::$name; }
    )*};
}
classes! {
    ProbeRegistry,
    LiveChanges,
    PageMembership,
    ChangeBlocks,
    RetentionStatistics,
    Commit,
    Flat,
    SourceAuthorizationRef,
    SourceMaps,
    SourceAuthorities,
    ActivityRegistry,
    ActivityDelivery,
    FaultPolicy,
    SpanExports,
    Reservation,
    Organizations,
    NestAuthority,
    FaultProgress,
    SourceIndex,
    ActivityHistory,
    Address,
    Cleanup,
    Nest,
    PendState,
    PendParent,
    LedgerAdmission,
    LedgerCharge,
    LedgerSerial,
    LedgerEntries,
    LedgerHeirs,
    BlockPages,
    BlockEpoch,
    BlockUses,
    BlockStages,
    Holds,
    HeldSources,
    SourceFlights,
    SourceGate,
    SourceBindings,
    SourceBlocks,
    SourceOrigins,
    SourcePhase,
    ProvenanceGrant,
    ProvenanceUses,
    Deadline,
    InterruptController,
    InterruptProgress,
    InterruptSources,
    InterruptSource,
    Scheduler,
    SchedulerAdmission,
    Shutdown,
    SignQueue,
    SignState,
    SignRouting,
    SignDelivery,
    WaitHolders,
    WaitSites,
    ControlTable,
    LifecycleDiagnostics,
    TerminationQueue,
    ReapQueue,
    Husks,
    TaskState,
    TaskReceipts,
    TaskResult,
    TaskJournal,
    TaskHeir,
    TaskRetirement,
    TaskSchedule,
    TaskExtensions,
    TaskMemory,
    Registry,
    TeamState,
    TeamTasks,
    TeamChildren,
    TeamTermination,
    TeamHeld,
    TeamExit,
    CloneQueue,
    BootMmio,
    BootRegions,
    BootNests,
    BlockPool,
    BlockPump,
    Tally,
    Bump,
    Frame,
    Spare,
    Controllers,
    Asid,
}
macro_rules! construction {
    ($($name:ident),* $(,)?) => {$ (
        impl sealed::Construction for $name {}
        impl Constructible for $name {}
    )*};
}
construction! {
    ProbeRegistry,
    LiveChanges,
    PageMembership,
    ChangeBlocks,
    RetentionStatistics,
    Flat,
    SourceAuthorizationRef,
    SourceMaps,
    SourceAuthorities,
    ActivityRegistry,
    ActivityDelivery,
    FaultPolicy,
    SpanExports,
    Reservation,
    Organizations,
    NestAuthority,
    FaultProgress,
    SourceIndex,
    ActivityHistory,
    Address,
    Cleanup,
    Nest,
    PendState,
    PendParent,
    LedgerAdmission,
    LedgerCharge,
    LedgerSerial,
    LedgerEntries,
    LedgerHeirs,
    BlockPages,
    BlockEpoch,
    BlockUses,
    BlockStages,
    Holds,
    HeldSources,
    SourceFlights,
    SourceGate,
    SourceBindings,
    SourceBlocks,
    SourceOrigins,
    SourcePhase,
    ProvenanceGrant,
    ProvenanceUses,
    Deadline,
    InterruptController,
    InterruptProgress,
    InterruptSources,
    InterruptSource,
    Scheduler,
    SchedulerAdmission,
    Shutdown,
    SignQueue,
    SignState,
    SignRouting,
    SignDelivery,
    WaitHolders,
    WaitSites,
    ControlTable,
    LifecycleDiagnostics,
    TerminationQueue,
    ReapQueue,
    Husks,
    TaskState,
    TaskReceipts,
    TaskResult,
    TaskJournal,
    TaskHeir,
    TaskRetirement,
    TaskSchedule,
    TaskExtensions,
    TaskMemory,
    Registry,
    TeamState,
    TeamTasks,
    TeamChildren,
    TeamTermination,
    TeamHeld,
    TeamExit,
    CloneQueue,
    BootMmio,
    BootRegions,
    BootNests,
    BlockPool,
    BlockPump,
    Tally,
    Bump,
    Frame,
    Spare,
    Controllers,
    Asid,
}
macro_rules! roots {
    ($($name:ident),* $(,)?) => {$ (
        impl sealed::Root for $name {}
        impl RootOrder for $name {}
    )*};
}
roots! {LedgerSerial,Commit,Nest,Flat}
impl sealed::Pair for Flat {}
impl PairOrder for Flat {}

// Explicit reachability closure, not a numeric rank and not a transitive
// blanket impl. This one table emits the static proof and Debug predicate.
macro_rules! relations {
    ($($parent:ident => [$($child:ident),* $(,)?]),* $(,)?) => {
        $($(impl sealed::Relation<$child> for $parent {}
             impl Before<$child> for $parent {})*)*
        #[cfg(debug_assertions)]
        static RELATIONS:[[bool;Class::ALL.len()];Class::ALL.len()]={
            let mut rows=[[false;Class::ALL.len()];Class::ALL.len()];
            $($(rows[Class::$parent as usize][Class::$child as usize]=true;)*)*
            rows
        };
        #[cfg(debug_assertions)]
        #[inline(always)]
        pub(crate) const fn before(parent:Class,child:Class) -> bool {
            RELATIONS[parent as usize][child as usize]
        }
    };
}
relations! {
    LiveChanges => [PageMembership,ChangeBlocks,RetentionStatistics,BlockPool,BlockPump,Tally,Frame],
    PageMembership => [RetentionStatistics,BlockPool,BlockPump,Tally,Frame],
    ChangeBlocks => [PageMembership,RetentionStatistics,BlockPool,BlockPump,Tally,Frame],
    Commit => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,SourceAuthorizationRef,SourceMaps,SourceAuthorities,ActivityRegistry,ActivityDelivery,FaultPolicy,SpanExports,Reservation,Organizations,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Address,Nest,PendState,PendParent,LedgerAdmission,LedgerCharge,LedgerEntries,LedgerHeirs,BlockPages,BlockEpoch,BlockUses,BlockStages,HeldSources,SourceFlights,SourceGate,SourceBindings,SourceBlocks,SourceOrigins,SourcePhase,ProvenanceGrant,ProvenanceUses,Deadline,InterruptController,InterruptProgress,InterruptSources,InterruptSource,Scheduler,SchedulerAdmission,SignQueue,SignState,SignRouting,SignDelivery,WaitHolders,WaitSites,ControlTable,TerminationQueue,Husks,TaskState,TaskReceipts,TaskResult,TaskJournal,TaskHeir,TaskSchedule,TaskExtensions,Registry,TeamState,TeamTasks,TeamChildren,TeamTermination,TeamHeld,TeamExit,CloneQueue,BlockPool,BlockPump,Tally,Frame,Controllers],
    Flat => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,SourceIndex,BlockPool,BlockPump,Tally,Frame],
    SourceAuthorities => [BlockPool,BlockPump,Tally,Frame],
    ActivityDelivery => [SignState,WaitSites,TaskState,BlockPool,BlockPump,Tally,Frame],
    FaultPolicy => [FaultProgress,SignQueue,SignState,SignDelivery,BlockPool,BlockPump,Tally,Frame],
    SpanExports => [BlockPool,BlockPump,Tally,Frame],
    Reservation => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,FaultProgress,SourceIndex,SignQueue,SignState,SignDelivery,BlockPool,BlockPump,Tally,Frame],
    Organizations => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,Reservation,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Nest,PendState,SourcePhase,SignQueue,SignState,SignDelivery,TaskJournal,BlockPool,BlockPump,Tally,Frame,Controllers],
    NestAuthority => [BlockPool,BlockPump,Tally,Frame],
    SourceIndex => [BlockPool,BlockPump,Tally,Frame],
    Address => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,Reservation,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Nest,PendState,SourcePhase,SignQueue,SignState,SignDelivery,TaskJournal,BlockPool,BlockPump,Tally,Frame,Controllers],
    Nest => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,Reservation,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,PendState,SourcePhase,SignQueue,SignState,SignDelivery,TaskJournal,BlockPool,BlockPump,Tally,Frame,Controllers],
    LedgerAdmission => [BlockPool,BlockPump,Tally,Frame],
    LedgerCharge => [LedgerAdmission,BlockPool,BlockPump,Tally,Frame],
    LedgerSerial => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Commit,Flat,SourceAuthorizationRef,SourceMaps,SourceAuthorities,ActivityRegistry,ActivityDelivery,FaultPolicy,SpanExports,Reservation,Organizations,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Address,Nest,PendState,PendParent,LedgerAdmission,LedgerCharge,LedgerEntries,LedgerHeirs,BlockPages,BlockEpoch,BlockUses,BlockStages,HeldSources,SourceFlights,SourceGate,SourceBindings,SourceBlocks,SourceOrigins,SourcePhase,ProvenanceGrant,ProvenanceUses,Deadline,InterruptController,InterruptProgress,InterruptSources,InterruptSource,Scheduler,SchedulerAdmission,SignQueue,SignState,SignRouting,SignDelivery,WaitHolders,WaitSites,ControlTable,TerminationQueue,Husks,TaskState,TaskReceipts,TaskResult,TaskJournal,TaskHeir,TaskSchedule,TaskExtensions,Registry,TeamState,TeamTasks,TeamChildren,TeamTermination,TeamHeld,TeamExit,CloneQueue,BlockPool,BlockPump,Tally,Frame,Controllers,Asid],
    LedgerEntries => [PendState,LedgerAdmission,LedgerCharge,CloneQueue,BlockPool,BlockPump,Tally,Frame],
    LedgerHeirs => [BlockPool,BlockPump,Tally,Frame],
    BlockPages => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,SourceIndex,BlockPool,BlockPump,Tally,Frame],
    BlockEpoch => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,SourceIndex,PendState,LedgerAdmission,LedgerCharge,LedgerEntries,BlockPages,TaskState,TeamState,CloneQueue,BlockPool,BlockPump,Tally,Frame],
    BlockUses => [BlockPool,BlockPump,Tally,Frame],
    BlockStages => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,SourceIndex,BlockPages,BlockPool,BlockPump,Tally,Frame],
    HeldSources => [BlockPool,BlockPump,Tally,Frame],
    SourceFlights => [BlockPool,BlockPump,Tally,Frame],
    SourceGate => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,ActivityRegistry,SourceIndex,PendState,LedgerAdmission,LedgerCharge,LedgerEntries,BlockPages,BlockEpoch,SourceFlights,SourceBlocks,SourcePhase,InterruptController,InterruptProgress,InterruptSources,InterruptSource,SignState,SignRouting,WaitSites,TaskState,TeamState,CloneQueue,BlockPool,BlockPump,Tally,Frame],
    SourceBindings => [BlockPool,BlockPump,Tally,Frame],
    SourceBlocks => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,SourceIndex,PendState,LedgerAdmission,LedgerCharge,LedgerEntries,BlockPages,BlockEpoch,TaskState,TeamState,CloneQueue,BlockPool,BlockPump,Tally,Frame],
    SourceOrigins => [BlockPool,BlockPump,Tally,Frame],
    SourcePhase => [PendState],
    ProvenanceGrant => [PendState],
    ProvenanceUses => [BlockPool,BlockPump,Tally,Frame],
    Deadline => [BlockPool,BlockPump,Tally,Frame],
    InterruptSources => [InterruptController,BlockPool,BlockPump,Tally,Frame],
    Scheduler => [TaskState,TaskSchedule,TaskExtensions,TeamState],
    SchedulerAdmission => [TaskState,TaskSchedule],
    SignQueue => [SignDelivery],
    SignRouting => [BlockPool,BlockPump,Tally,Frame],
    WaitHolders => [BlockPool,BlockPump,Tally,Frame],
    WaitSites => [TaskState,BlockPool,BlockPump,Tally,Frame],
    ControlTable => [BlockPool,BlockPump,Tally,Frame],
    TerminationQueue => [TeamState,TeamTermination],
    ReapQueue => [TaskState],
    Husks => [TaskState],
    TaskReceipts => [BlockPool,BlockPump,Tally,Frame],
    TaskJournal => [Frame],
    TaskHeir => [BlockPool,BlockPump,Tally,Frame],
    Registry => [TeamState,BlockPool,BlockPump,Tally,Frame],
    TeamTasks => [TaskState,TaskReceipts,Registry,TeamState,TeamHeld,BlockPool,BlockPump,Tally,Frame],
    TeamChildren => [TaskHeir,BlockPool,BlockPump,Tally,Frame],
    TeamHeld => [Registry,TeamState,BlockPool,BlockPump,Tally,Frame],
    BootMmio => [BlockPool,BlockPump,Tally,Frame],
    BootRegions => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Commit,Flat,SourceAuthorizationRef,SourceMaps,SourceAuthorities,ActivityRegistry,ActivityDelivery,FaultPolicy,SpanExports,Reservation,Organizations,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Address,Nest,PendState,PendParent,LedgerAdmission,LedgerCharge,LedgerEntries,LedgerHeirs,BlockPages,BlockEpoch,BlockUses,BlockStages,HeldSources,SourceFlights,SourceGate,SourceBindings,SourceBlocks,SourceOrigins,SourcePhase,ProvenanceGrant,ProvenanceUses,Deadline,InterruptController,InterruptProgress,InterruptSources,InterruptSource,Scheduler,SchedulerAdmission,SignQueue,SignState,SignRouting,SignDelivery,WaitHolders,WaitSites,ControlTable,TerminationQueue,Husks,TaskState,TaskReceipts,TaskResult,TaskJournal,TaskHeir,TaskSchedule,TaskExtensions,Registry,TeamState,TeamTasks,TeamChildren,TeamTermination,TeamHeld,TeamExit,CloneQueue,BlockPool,BlockPump,Tally,Frame,Controllers],
    BootNests => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,Reservation,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Nest,PendState,SourcePhase,SignQueue,SignState,SignDelivery,TaskJournal,BlockPool,BlockPump,Tally,Frame,Controllers],
    // Queue growth and payload release may allocate/free while CloneQueue is
    // held. Allocator leaves never acquire CloneQueue or LedgerSerial.
    CloneQueue => [BlockPool,BlockPump,Tally,Frame],
    BlockPool => [Tally,Frame],
    Controllers => [PageMembership,RetentionStatistics,BlockPool,BlockPump,Tally,Frame],
    Asid => [Frame],
    Cleanup => [LiveChanges,PageMembership,ChangeBlocks,RetentionStatistics,Flat,FaultPolicy,Reservation,NestAuthority,FaultProgress,SourceIndex,ActivityHistory,Nest,PendState,SourcePhase,SignQueue,SignState,SignDelivery,TaskJournal,BlockPool,BlockPump,Tally,Frame,Controllers],
}

// A full closure has no self edges. Longest-path relaxation also rejects a
// cycle and proves the fixed Debug held capacity sufficient (plus two peers).
#[cfg(debug_assertions)]
pub(crate) const MAX_DEPTH: usize = {
    let n = Class::ALL.len();
    let mut incoming = [0usize; Class::ALL.len()];
    let mut depth = [1usize; Class::ALL.len()];
    let mut done = [false; Class::ALL.len()];
    let mut p = 0;
    let mut c;
    while p < n {
        c = 0;
        while c < n {
            assert!(p != c || !before(Class::ALL[p], Class::ALL[c]));
            if before(Class::ALL[p], Class::ALL[c]) {
                incoming[c] += 1;
            }
            c += 1;
        }
        p += 1;
    }
    let (mut visited, mut maximum) = (0, 0);
    while visited < n {
        p = 0;
        while p < n && (done[p] || incoming[p] != 0) {
            p += 1;
        }
        assert!(p < n, "lock order contains a cycle");
        done[p] = true;
        visited += 1;
        if depth[p] > maximum {
            maximum = depth[p];
        }
        c = 0;
        while c < n {
            if before(Class::ALL[p], Class::ALL[c]) {
                incoming[c] -= 1;
                if depth[c] <= depth[p] {
                    depth[c] = depth[p] + 1;
                }
            }
            c += 1;
        }
    }
    maximum
};
