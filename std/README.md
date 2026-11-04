# kernel-std / kernel-std-rt experimental prototype

Status: experimental isolation prototype, not a claimed production cutover and
not an accepted user decision. This directory is an independent Cargo workspace
under `std/`, like `api/`'s independent workspace precedent. It does not edit the
root kernel manifest, root kernel source, production graph, `api` source, or
existing consumers.

## Intended prototype graph

```text
kernel-std-rt -> kernel-std -> kernel-api
kernel-std -> kernel-std-ipc -> kernel-std-resource -> kernel-api-env
kernel-std-ipc -> kernel-api-wire
```

`kernel-std-rt` is an opt-in binary-side package. The runtime-free fixture
(`std/fixtures/std-only`) intentionally has no dependency on it.

## External adapter drafts and integration seam

Coordinator-verified unmerged adapter drafts are:

| Draft | PR | Commit | Kernel pin |
|---|---:|---:|---:|
| env | <https://github.com/shuyra-arche/env/pull/1> | `1c24f86ab9e988826489c586d81e7eb4a8c2a109` | `a9986c82127cf25183bf5c30e7d42e4a9376b734` |
| wire | <https://github.com/shuyra-arche/wire/pull/1> | `2cf20aea1a6de0751e6a70288ef72fcc64629aef` | `a9986c82127cf25183bf5c30e7d42e4a9376b734` |
| mold | <https://github.com/shuyra-arche/mold/pull/1> | `3cf74899c798f2e2750b2367a4ff80008e9e054f` | `a9986c82127cf25183bf5c30e7d42e4a9376b734` |

These are staging/unmerged drafts and are not used by this prototype. This
workspace depends directly on the canonical PATH packages under `api/`, not on
the external adapter branches. Matching source contents or pin names does not
by itself make a PATH package and a Git package the same Cargo owner or the
same future release identity.

Legacy `mold` entry expansion still targets `execution::boot::entry`. This
prototype deliberately does not use that hardwired execution path: its
`kernel_std_rt::entry!` macro is a separate hygienic declaration. Future
consumer migration must explicitly choose this rt seam, not re-export the old
execution runtime path.

Before any mixed adapter graph is claimed, one coordinated canonical owner is
needed through aligned release pins, or an isolated patch plus explicit
path/Git type-identity validation. This prototype does not claim
adapter-composed validation yet; that remains a pending integration task.

## Ownership split

### `kernel-api` owns

ABI definitions, wire vocabulary/codec, `ecall::trap`, `make_fail`, exit reason
constants, permissions, and handle types. It stays runtime-free.

### `kernel-std` owns reusable mechanisms

- `lock`: no global state.
- `memory`: reference/view mechanism plus the imported heap state machine.
- `unit::task`: spawn/join machinery and the sole `DETACHED` detached-completion
  registry.
- `unit::tls`: the sole TLS state machine (`BLOCKS`, `OWED`, `PENDING`, slot
  table).
- `room`: `park`, `reap`, `wait`, `wake`.
- Re-exported `resource` and `ipc` mechanism packages.

`kernel-std` must not define `_start`, `save_args`, `tls_bootstrap`,
`clean_ret`, `#[panic_handler]`, or `#[global_allocator]`.

### `kernel-std-rt` owns singleton runtime effects

- `_start` assembly.
- `save_args` / `args()` and the `ARGS` / `ARG_COUNT` statics.
- `tls_bootstrap` symbol that calls `kernel_std::execution::bootstrap_tls()`
  exactly once.
- `clean_ret`-side entry path via the hygienic `entry!(path)` macro.
- `#[panic_handler]` using `kernel_std::execution::room::reap(EXIT_PANIC, note)`.
- `#[global_allocator]` over `kernel_std::execution::memory::HeapAllocator`.
- `link.ld` and `links = "kernel_std_rt"` / `cargo::metadata=linker_script`
  build metadata.

## Runtime features

`kernel-std-rt` has `default = []` and three independent features:

- `entry`: emits `_start`, `save_args`, `tls_bootstrap`, and the `entry!` macro
  that defines `clean_ret`.
- `panic-handler`: emits `#[panic_handler]`.
- `global-allocator`: registers the single std heap as `#[global_allocator]`.

The std-only fixture owns all three registrations itself and proves std is not
silently depending on rt.

Here, "runtime-free" means the absence of automatic startup registration, not
the absence of every exported mechanism symbol. `kernel-std` does not register
`_start`, `save_args`, `tls_bootstrap`, `clean_ret`, a panic handler, or a
global allocator. The legacy exported `execution::unit::tls::bootstrap` symbol
(and the task trampoline) remain explicit callable mechanisms, not automatic
startup paths; a future stricter export policy may decide whether to prune or
rename those exported seams.

## Preserved startup sequence and inherited ABI quirk

The `_start` sequence is preserved verbatim:

```text
call save_args
call tls_bootstrap
addi sp, sp, -8
mv   a0, sp
call clean_ret
```

This sequence captures `a0`/`a1` as the argument pointer/count, performs TLS
bootstrap, then calls `clean_ret`. The 8-byte stack adjustment and `a0=sp`
contract are inherited as-is. No silent ABI or stack-alignment correction is
made in this prototype; RISC-V calling-convention stack alignment around this
handoff remains an open interface question.

## State authority and cleanup

| State/effect | Sole authority | Allowed to others |
|---|---:|---|
| `ARGS` / `ARG_COUNT` | `kernel-std-rt` | readers through `args()` only |
| `_start`, `tls_bootstrap`, `clean_ret` | `kernel-std-rt` | std must not export duplicates |
| panic handler | `kernel-std-rt` | std must not define one |
| global allocator registration | `kernel-std-rt` | std owns allocator state but not the singleton binding |
| TLS slot table / `OWED` / `PENDING` | `kernel_std::execution::unit::tls` | rt invokes bootstrap exactly once; task trampoline invokes allocate/deallocate; `finished` invokes `reap_task` |
| heap region table / `OWED` / `PENDING` | `kernel_std::execution::memory::allocator` | rt only installs `#[global_allocator]`; no second heap state |
| `DETACHED` join registry | `kernel_std::execution::unit::task` | no rt/resource/ipc access |
| IPC session lifecycle state | `kernel_std::ipc::session::State` per `Arc` | rt must not gate, seal, or own session state |

Per-object RAII responsibilities from imported source remain unchanged:
resource `Capability`, `Loan`, `Reply`, `Dock::shut`, `Port::shut`, IPC
`session::Owned`, and `session::CallGuard`.

## Exit/panic mapping is prototype-only

This prototype's rt panic calls `room::reap(EXIT_PANIC)` for the current task,
and `execution::task::Join` returns a Rust value and panics if a child terminates
before publishing that value. These are not Rust `std::process::exit`,
`std::thread::JoinHandle` panic-payload, or unwinding semantics. A future
platform std must explicitly map task/team/process ownership, cleanup, and
panic strategy before claiming std-like behavior.

## Relationship to a future Rust platform std / PAL / Cargo

This is a no_std OS mechanism prototype, not Rust's actual `std` platform crate,
PAL, or Cargo. It does not implement file/service I/O, error/threading/unwind,
platform process semantics, or a native Cargo/rustc running on the target.

Target constraints observed here:

- The pinned RISC-V target uses `rust-lld` and no target `libc` or `cc` crate
  dependency. This is about this prototype's target graph, not a claim that all
  third-party native crates or `build.rs` files will work without a C compiler.
- `extern "C"` items are retained for assembly/kernel ABI handoff; that is a C
  ABI calling convention, not a libc or target C compiler dependency.
- Host builds may use the host linker for proc macros and host tests; that is
  separate from the RISC-V target requirement.

A future platform std may consume the runtime-free mechanisms here and select
its own allocator/panic/start registrations. It must still settle:

- current custom TLS block (`TLS_SIZE = PAGE_SIZE`, raw page allocation, `tp`
  write) versus compiler TLS layout (`.tdata`/`.tbss`/`tp`);
- thread keys/destructors and thread entry/exit hooks;
- process/thread panic and exit payload semantics;
- service/file I/O, error mapping, unwind, and scheduler integration.

Pinned `rustc --print cfg` reports the RISC-V target has `target_os = "none"`
and `target_has_threads` but no `target_thread_local`. The installed rust-src
PAL selection falls through to unsupported for this `target_os = "none"` target.
The current `link.ld` has no `.tdata`/`.tbss` template handling. Therefore Rust
std/native TLS compatibility is not proven by this prototype.

## Open decisions

1. IPC kernel vs user protocol boundary.
2. Initial TLS vs task TLS, and compiler TLS compatibility.
3. Exit report location: this prototype keeps `Exit`/`Report`/`finish`/`entry`
   in `kernel-std-rt`; the namer recommendation was `kernel_std::exit`.
4. Runtime feature defaults and binary opt-in policy.
5. Declarative `entry!` macro seam vs a future hygienic proc macro; this
   prototype deliberately does not add a proc-macro crate.
6. Future platform std/PAL ownership and exact std-like semantics.

## Verification

See `VALIDATION.md` for actual checks, counts, and limitations.

The ABI identity test is representative rather than exhaustive. It proves exact
type identity for directly reexported `kernel_std::env`/`wire` facade types such
as `TaskId`, `TeamId`, `ViewId`, `PieToken`, `FlatReference`, `Reason`, and
`Permission` through function signatures, not merely byte equality. It does not
claim an exhaustive all-public-type identity matrix.
