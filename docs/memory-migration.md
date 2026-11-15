# Native memory source migration

ABI 11 accepts bounded Flat/Nest descriptors from `abi::memory`, not the old
Cell class 2 or Flat/Nest register classes 12/13. Share register slots 0, 2 and 3
are also retired; slots 1 (Delete) and 4 (Derive) remain callable. Retired numbers
remain rejected and are not reused. ABI and Ledger stay at version 11; Capsule format remains version 2.

| Removed surface | Reachability / consumer disposition |
| --- | --- |
| Cell register operations and Client wrappers | Rejected before aggregate dispatch; remove Cell dispatch and its Slot adapters |
| Flat/Nest register operations and Client wrappers | Rejected before aggregate dispatch; keep native descriptor handlers |
| Share Create/Open/Shut wrappers | Rejected by v11; native derivation/Clone/boot retain the same source owner |
| Cell publication errors | Live Memo consumers use internal Memo admission errors; Page maintenance returns RetainError |
| CellLevel, Page and placement geometry | Live native memory, boot and allocator consumers remain |
| SourceAuthorization / Hold tracking | Native cross-Task association, cleanup and Clone still require these owners |

This is a source API cleanup. Consumers must use Client `memory::{flat,nest}`
and bounded native descriptors; no old-name compatibility layer is provided.
Historical release SHAs and their ABI 10 documentation describe the old source
surface and remain historical facts.
