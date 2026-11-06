# Kernel API

Kernel owns the RV64 register ABI, capability vocabulary, memory descriptors and
startup ledger. Client executes operations; Server decodes and dispatches them.
Operation contracts perform no trap, retries, rollback or service communication.

| Package | Responsibility |
| --- | --- |
| `kernel-api-env` | Operation contracts, register encodings, failures, handles, permissions and concrete Kernel byte layouts |
| `kernel-api-macros` | One `envcall` declaration macro for operations and their error domains |
| `kernel-api` | Re-export umbrella for Env |
| `kernel-client` | Named call functions, native trap and reusable task/memory mechanisms |

```text
kernel-api (umbrella) ────────────> kernel-api-env
kernel-client ───────────────────> kernel-api-env
kernel (Server) / allocator ─────> kernel-api-env
kernel-api-env ── build-time ────> kernel-api-macros
```

## Operation declarations

Each call domain declares its operations and failures together. For example:

```rust,ignore
#[kernel_api_macros::envcall(class = 2, call = MemoryCall)]
pub mod operations {
    #[error_domain]
    pub enum MemoryFail {
        Denied = -1,
        OoM = -2,
    }

    #[operation(slot = 6, output = (), error = MemoryFail)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Release {
        pub reference: super::FlatReference,
    }
}
```

The request value carries the inputs. `Operation` associates its success and
failure types. `RegisterCall` defines its constant slot, six-word packing and
request decoding. Returning operations also implement `KernelCall`, which
provides pure result decoding. A fallible operation's error type remains
`erra::Error<DeclaredFailure>`; `infallible` operations use `Infallible` and retain
all success bits, including the high bit of the first register.

Every operation declares an explicit 32-bit slot; its domain declares a 32-bit
class. `Wire::WORDS` describes each input's exact register consumption. The
macro checks that their sum does not exceed six. `reply = 3` selects three-word
success decoding; the default uses two words. Error codes are explicit,
consecutive negative values starting at -1 within each domain; `#[busy]` marks
its optional not-ready result. The macro generates conversions, `FailCode` and
the standard error traits, including diagnostic `Display`.

A `nonreturning` operation has no output, error or result decoder. It implements
`RegisterCall` and `TerminalCall`, and cannot enter the normal Client invocation.
`Reap` uses this contract. Client owns the terminal trap and the fault path if
that trap unexpectedly returns; the fault path uses neither TLS nor panic.

The macro also generates a named request enum such as `MemoryCall` for Server
matching. Its variants retain their named fields; packing and slot constants
belong to operation structs. `from_wire` checks both class
and slot before decoding inputs. Returning calls decode directly to their own
success type; there is no domain-wide return enum or runtime result extraction.
Client owns the named function signatures and chooses execution independently.

`operation` and `error_domain` are markers consumed by `envcall`, not separate
macros. Auxiliary error-only declarations use `#[envcall]` without a class or
call enum. The macro accepts inline modules with imports, error enums and
operation structs. Operations are public, concrete named-field or unit structs;
generics and conditional fields/operations are rejected. Generated `crate::wire`
and `crate::ecall` paths intentionally target the canonical Kernel API; this is
not a general-purpose external code-generation library.

## Concrete byte layouts

`Placement` and `ReferenceRange` are 24-byte little-endian descriptors. A ledger
`Name` uses 16 bytes, `Header` uses 8, and `Entry` uses 32. Their inherent methods
encode these specific ABI layouts without a generic byte-codec dependency.

`store_entries` writes a startup ledger into caller-provided capacity and returns
its used length. `Entries::new(&bytes[..used])` validates the complete ledger:
version, records, reserved bytes, unique names and no trailing bytes.

Generic byte codecs, service protocols and their macro generators are outside
Kernel. `env::wire` is the register ABI vocabulary; `Mark` is a capability ABI
value. No Weave dependency or adapter is required.

## Compatibility and validation

Call numbers, register and concrete byte layouts, failure codes, and the 66 named
Client entry signatures are preserved. `room::reap` retains its separate `!`
interface. Source users of the former derives, `*CallRet`, aggregate return
decoding or the earlier `KernelCall` trait must use the operation contracts.
Aggregate request enums no longer provide `slot` or `pack`; use operation types.
Unused success-decoder implementations and `PieKind` byte-codec helpers have
been removed. `Wire` implementations must provide their exact `WORDS` count. Consumers of the
removed generic communication exports must supply those through their own
communication library before updating Kernel. Older pinned revisions remain
independent; external consumer repositories are not migrated here.

Server and allocator use the same workspace API source. Consumer workspaces
must coordinate source identity because path and Git copies define distinct
Rust types.

From the repository root (substitute the Windows host target when applicable):

```sh
cargo test --locked -p kernel-api-env --tests --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-api-env --doc --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-api-macros --test envcall_boundary --target x86_64-unknown-linux-gnu
cargo test --locked -p kernel-client --test calls --target x86_64-unknown-linux-gnu
```

Host checks cover fixed bytes, all 67 published operation slots, typed results,
request decoding, declaration diagnostics and rejected contract combinations.
They never execute a native trap. See the root [README](../README.md) for RV64
build commands and the separate responsibility for guest execution validation.
