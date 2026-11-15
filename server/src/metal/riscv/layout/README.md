# Frozen Kernel layout and physical access

Boot admits platform memory before Bump initialization, chooses MMU geometry,
and freezes `KernelLayout` before Frame/Block initialization and secondary-Hart
mapping/stack preparation. `plan.rs` owns the window geometry. Nest and Boot
consume that plan; no mutable free-space index duplicates Occupancy.

| Geometry | Exclusive User ceiling | High canonical base |
| --- | --- | --- |
| Sv39 | `0x4000000000` | `0xffffffc000000000` |
| Sv48 | `0x800000000000` | `0xffff800000000000` |
| Sv57 | `0x100000000000000` | `0xff00000000000000` |

For every geometry the order is Direct, Resource, Trap Stack, Team Frame,
Hart Frame, Reserved, Trampoline. Direct's reserved VA envelope spans admitted
RAM's maximum end, but only the admitted identity/direct RAM intervals receive
mappings: a VA envelope is not proof that a physical hole is accessible.
Resource ends at the fixed Trap Stack base. Normal starts at `0x10000` and
ends below the User ceiling; existing image/boot occupancy further excludes
used addresses. All windows use inclusive final bytes so the top page cannot
overflow an exclusive usize end. Construction rejects unsupported geometry,
zero/unaligned/excessive RAM end, overlapping windows and 0 or >256 Harts.

Team Frame reserves 64 MiB; the page-table-friendly Hart aperture stays 2 MiB.
Reserved is 2 MiB minus the final 4 KiB Trampoline page. Trampoline stays at
`0xfffffffffffff000`. Hart capacity decreases from 4096 to the same 256 admitted
by the public boot contract; Trap Stack VA reservation decreases 256 ->16 MiB.
This does not map stacks for unused slots.

| Admitted Harts | Static PER_HART bytes at 64 bytes/entry | Mapped trap body bytes (60 KiB/Hart) | Hart frame bytes (4 KiB/Hart) |
| --- | --- | --- | --- |
| 1 | 16384 | 61440 | 4096 |
| 2 | 16384 | 122880 | 8192 |
| 4 | 16384 | 245760 | 16384 |
| 256 | 16384 | 15728640 | 1048576 |

Static PER_HART was 262144 bytes. The table excludes page tables, allocator
rounding and Kernel work/bootstrap stacks; these must be separately charged
by #186. Each 64 KiB trap slot preserves its absent 4 KiB lower guard. Host
geometry tests cover every listed mode and 1/2/4/256 Harts; local QEMU execution
covers 1/4 Harts, not 256 physical CPUs or physical cache behavior.

For free-region page counts p_i, Frame allocates sum(p_i) Option<u8> metadata
entries, one Buddy owner per region and sum(floor(log2(p_i))+1) free-list heads.
Tally and optional diagnostics index the same compact page geometry. The
immutable Pages view borrows boot regions and allocates no index. A 1 TiB hole
between 5 actual pages therefore still has 5 metadata indices, not 268435456
hole entries. Host sparse-region tests verify the count and both directions
of the mapping; this is a geometry/cost statement, not an allocator-throughput
measurement. Exact metadata byte budgets use the target's size_of values and
actual Vec capacities. All metadata allocation finishes before freezing the
final Bump frontier and publishing free links; metadata pages never re-enter
the free list. Buddy physical-XOR merges remain inside one admitted region.

`metal::physical::{read,write}` centralizes changed Frame/Buddy, raw PTE and
Task/scheduler context accesses. Conversion checks alignment and admitted
identity coverage; write also excludes Kernel Text/ROData aliases. The caller
still proves object custody, initialization, synchronization and retirement.
MMIO uses its separately admitted aperture. Text remains RX and ROData R across
identity/direct aliases; ordinary RAM mappings do not grant execute permission.
No-map holes and unadmitted MMIO never become ordinary RAM by conversion.

Identity mappings remain intentionally available for boot/SBI, secondary Harts,
trampoline entry, page tables, allocator receipts, IOMMU support and diagnostics.
Historical casts outside the migrated paths still need a path-by-path audit
before identity mappings can be narrowed. This change neither removes identity
mapping nor treats a successful address conversion as authorization. Hardware
retirement and physical cache performance remain separate acceptance gates.
