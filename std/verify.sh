#!/usr/bin/env bash
set -euo pipefail

TOOLCHAIN="nightly-2026-09-04"
export RUSTUP_TOOLCHAIN="$TOOLCHAIN"

STD="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$STD/.." && pwd)"
SOURCE_ROOT="${SOURCE_ROOT:-}"
export CARGO_TARGET_DIR="$STD/target"
cd "$REPO"

HOST_TARGET="x86_64-unknown-linux-gnu"
RV_TARGET="riscv64gc-unknown-none-elf"

RT_LINK_SHA256="e9ba425282fd6484497ef41eb9151da29c069f3c2c85d5d91a16d5cb72fcfe60"

SYSROOT="$(rustc --print sysroot)"
NM="$SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-nm"
READOBJ="$SYSROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-readobj"

info() { printf '== %s\n' "$*"; }

run_host_tests() {
  local manifest="$1"
  info "host test manifest $manifest"
  cargo test --locked --manifest-path "$manifest" --target "$HOST_TARGET"
}

info "pinned toolchain $(rustc --version)"

info "std workspace rv library builds (debug default)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" \
  -p kernel-std -p kernel-std-execution -p kernel-std-resource -p kernel-std-ipc -p kernel-std-rt

info "std workspace rv library builds (release default)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" --release \
  -p kernel-std -p kernel-std-execution -p kernel-std-resource -p kernel-std-ipc -p kernel-std-rt

symbols() {
  local file="$1"
  local tmp
  [[ -f "$file" ]] || { echo "missing symbol file: $file" >&2; exit 1; }
  tmp="$(mktemp)"
  "$NM" --defined-only "$file" >"$tmp" 2>&1 || {
    echo "llvm-nm failed for $file" >&2
    cat "$tmp" >&2
    rm -f "$tmp"
    exit 1
  }
  awk '/^[0-9a-fA-F]+ [A-Za-z] / {print $3}' "$tmp"
  rm -f "$tmp"
}

contains_any() {
  local file="$1"
  local pattern="$2"
  local listing
  listing="$(symbols "$file")" || exit 1
  grep -E "$pattern" <<<"$listing" >/dev/null
}

assert_no_registrations() {
  local file="$1"
  if contains_any "$file" '^(_start|save_args|tls_bootstrap|clean_ret)$|rust_begin_unwind|rust_begin_panic|___rust_alloc|__rust_alloc|__rust_dealloc|__rust_realloc|__rust_alloc_zeroed|__rg_alloc|__rg_dealloc|__rg_realloc|__rg_oom'; then
    echo "registration symbols leaked from $file" >&2
    symbols "$file" | grep -E '^(_start|save_args|tls_bootstrap|clean_ret)$|rust_begin_unwind|rust_begin_panic|___rust_alloc|__rust_alloc|__rust_dealloc|__rust_realloc|__rust_alloc_zeroed|__rg_alloc|__rg_dealloc|__rg_realloc|__rg_oom' >&2 || true
    exit 1
  fi
}

for profile in debug release; do
  rdir="$STD/target/$RV_TARGET/$profile"
  for pkg in kernel_std kernel_std_execution kernel_std_resource kernel_std_ipc; do
    info "mechanism rlib symbol scan $profile lib${pkg}.rlib"
    assert_no_registrations "$rdir/lib${pkg}.rlib"
  done

  archive_count=0
  while IFS= read -r -d '' archive; do
    base="$(basename "$archive")"
    if [[ "$base" == *kernel_std_rt* ]]; then
      continue
    fi
    archive_count=$((archive_count + 1))
    info "target dependency rlib symbol scan $profile $base"
    assert_no_registrations "$archive"
  done < <(find "$rdir" -type f -name '*.rlib' -print0)
  (( archive_count > 0 ))

  info "runtime default rlib symbol scan $profile libkernel_std_rt.rlib"
  assert_no_registrations "$rdir/libkernel_std_rt.rlib"
done

info "runtime all-feature rv library builds (debug)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" \
  -p kernel-std-rt --features entry,panic-handler,global-allocator
info "runtime all-feature rv library builds (release)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" --release \
  -p kernel-std-rt --features entry,panic-handler,global-allocator

for profile in debug release; do
  rt="$STD/target/$RV_TARGET/$profile/libkernel_std_rt.rlib"
  info "runtime all-feature rlib symbol scan $profile"
  for name in _start save_args tls_bootstrap; do
    contains_any "$rt" "^${name}$"
  done
  contains_any "$rt" 'rust_begin_unwind|rust_begin_panic'
  contains_any "$rt" '___rust_alloc|__rust_alloc|__rust_dealloc|__rust_realloc|__rust_alloc_zeroed|__rg_alloc|__rg_dealloc|__rg_realloc|__rg_oom'
  if contains_any "$rt" '^clean_ret$'; then
    echo "clean_ret must be generated in the consuming binary, not in the rt rlib: $rt" >&2
    exit 1
  fi
done

info "standalone rv fixture builds (debug)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" \
  -p kernel-std-std-only-elf -p kernel-std-rt-opt-in-elf
info "standalone rv fixture builds (release)"
cargo build --locked --manifest-path "$STD/Cargo.toml" --target "$RV_TARGET" --release \
  -p kernel-std-std-only-elf -p kernel-std-rt-opt-in-elf

for profile in debug release; do
  bindir="$STD/target/$RV_TARGET/$profile"
  std_elf="$bindir/std-only"
  rt_elf="$bindir/rt-opt-in"

  info "runtime-free ELF symbol scan $profile"
  contains_any "$std_elf" '^_start$'
  if contains_any "$std_elf" '^(save_args|tls_bootstrap|clean_ret)$'; then
    echo "runtime-free std-only fixture unexpectedly acquired rt symbols" >&2
    exit 1
  fi

  info "runtime opt-in ELF symbol scan $profile"
  for name in _start save_args tls_bootstrap clean_ret; do
    contains_any "$rt_elf" "^${name}$"
  done

  info "runtime opt-in ELF entry and PT_LOAD alignment $profile"
  start_addr="$("$NM" --defined-only "$rt_elf" | awk '$3=="_start"{print $1}' | head -n1)"
  [[ -n "$start_addr" ]] || { echo "missing _start symbol" >&2; exit 1; }
  python3 - "$rt_elf" "$((16#$start_addr))" <<'PY'
import struct
import sys

path, expected_entry = sys.argv[1], int(sys.argv[2])
with open(path, "rb") as f:
    ident = f.read(16)
    assert ident[:4] == b"\x7fELF", "not an ELF file"
    assert ident[4] == 2 and ident[5] == 1, "not a 64-bit little-endian ELF"
    ehdr = f.read(48)
    (
        e_type,
        e_machine,
        e_version,
        e_entry,
        e_phoff,
        _e_shoff,
        _e_flags,
        _e_ehsize,
        e_phentsize,
        e_phnum,
        _e_shentsize,
        _e_shnum,
        _e_shstrndx,
    ) = struct.unpack_from("<HHIQQQIHHHHHH", ehdr, 0)
    assert e_type == 2, f"not an executable: e_type={e_type}"
    assert e_machine == 0xF3, f"not RISC-V: e_machine={e_machine:#x}"
    assert e_entry == expected_entry, f"entry {e_entry:#x} != _start {expected_entry:#x}"
    f.seek(e_phoff)
    loads = []
    for _ in range(e_phnum):
        ph = f.read(e_phentsize)
        p_type, p_flags, p_offset, p_vaddr, p_paddr, p_filesz, p_memsz, p_align = struct.unpack_from(
            "<IIQQQQQQ", ph, 0
        )
        if p_type == 1:
            loads.append((p_offset, p_vaddr, p_align))
    assert loads, "no PT_LOAD segments"
    for offset, vaddr, align in loads:
        assert offset % 4096 == 0, f"PT_LOAD p_offset not page-aligned: {offset:#x}"
        assert vaddr % 4096 == 0, f"PT_LOAD p_vaddr not page-aligned: {vaddr:#x}"
        assert (offset - vaddr) % 4096 == 0, f"PT_LOAD p_offset/p_vaddr not congruent: {offset:#x}/{vaddr:#x}"
        assert align != 0 and align % 4096 == 0, f"PT_LOAD p_align not page-aligned: {align}"
PY
done

info "runtime linker metadata identity"
printf '%s  %s\n' "$RT_LINK_SHA256" "$STD/rt/link.ld" | sha256sum -c - >/dev/null
grep -q '^links = "kernel_std_rt"' "$STD/rt/Cargo.toml"
grep -q 'cargo::metadata=linker_script=' "$STD/rt/build.rs"

info "workspace dependency identity and no libc/cc target graph"
meta_json="$(mktemp)"
cargo metadata --locked --manifest-path "$STD/Cargo.toml" --format-version 1 >"$meta_json"
python3 - "$meta_json" "$REPO" <<'PY'
import json
import os
import sys

repo = os.path.abspath(sys.argv[2])
meta = json.load(open(sys.argv[1]))
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

canonical = {"kernel-api-env": 0, "kernel-api-wire": 0, "kernel-api-macros": 0}
old_names = {"env", "wire", "mold"}
libc_names = {"libc", "cc"}

for p in meta["packages"]:
    if p["name"] in canonical:
        canonical[p["name"]] += 1
        path = os.path.abspath(p["manifest_path"])
        expected = os.path.join(repo, "api", p["name"].removeprefix("kernel-api-"))
        if path != os.path.join(expected, "Cargo.toml"):
            raise SystemExit(f"canonical package {p['name']} has unexpected manifest path: {path}")
    if p["name"] in old_names:
        raise SystemExit(f"old package identity {p['name']} present in std workspace graph")
    if p["name"] in libc_names:
        raise SystemExit(f"unexpected target-graph crate {p['name']}: {p['id']}")

for name, count in canonical.items():
    if count != 1:
        raise SystemExit(f"expected exactly one {name}, found {count}")

workspace_ids = set(meta["workspace_members"])
for pkg_id in workspace_ids:
    pkg = packages[pkg_id]
    node = nodes.get(pkg_id)
    if node is None:
        raise SystemExit(f"workspace package {pkg_id} missing from resolve graph")
    for dep in node.get("deps", []):
        dep_pkg = packages.get(dep["pkg"])
        if dep_pkg is None:
            continue
        if dep_pkg["name"] in old_names:
            raise SystemExit(f"{pkg['name']} depends on old package identity {dep_pkg['name']}")
        if dep_pkg["name"] in canonical and dep["name"] not in ("env", "wire", "mold"):
            raise SystemExit(f"{pkg['name']} has unexpected dependency rename {dep['name']} for {dep_pkg['name']}")
        if dep_pkg["name"] in ("kernel-std-execution", "kernel-std-resource", "kernel-std-ipc"):
            path = os.path.abspath(dep_pkg["manifest_path"])
            if not path.startswith(os.path.join(repo, "std") + os.sep):
                raise SystemExit(f"{dep_pkg['name']} resolves outside std: {path}")

start_ids = [
    p["id"]
    for p in packages.values()
    if p["name"] == "kernel-std-std-only-elf" and p["id"] in workspace_ids
]
assert len(start_ids) == 1, f"std-only fixture package not unique: {start_ids}"
seen = set()
stack = [start_ids[0]]
while stack:
    current = stack.pop()
    if current in seen:
        continue
    seen.add(current)
    node = nodes[current]
    for dep in node.get("deps", []):
        dep_pkg = packages[dep["pkg"]]
        if dep_pkg["name"] == "kernel-std-rt":
            raise SystemExit("std-only fixture resolves kernel-std-rt")
        if dep_pkg["id"] not in seen:
            stack.append(dep_pkg["id"])

print("cargo metadata identity checks passed")
PY
rm -f "$meta_json"

info "host test suites"
run_host_tests "$STD/execution/tests/Cargo.toml"
run_host_tests "$STD/execution/tests/unit/Cargo.toml"
run_host_tests "$STD/resource/tests/capability/Cargo.toml"
run_host_tests "$STD/ipc/tests/host/Cargo.toml"
run_host_tests "$STD/ipc/tests/rack-reader-host/Cargo.toml"
run_host_tests "$STD/ipc/tests/session-establish/Cargo.toml"
run_host_tests "$STD/ipc/tests/session-exchange/Cargo.toml"
run_host_tests "$STD/rt/tests/args-host/Cargo.toml"
run_host_tests "$STD/tests/abi-identity/Cargo.toml"

info "verification complete"
