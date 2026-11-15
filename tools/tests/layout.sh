#!/usr/bin/env bash
set -euo pipefail
checker=$(cd "$(dirname "$0")/.." && pwd)/check-layout.sh
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cd "$scratch"
git init -q
mkdir -p api/src/memory server/tests/target/cell/space 'tools/with space'
touch api/src/memory/mod.rs api/src/memory/activity.rs api/src/other.rs
touch server/tests/target/cell/space/mod.rs 'tools/with space/main.rs'
git add .
bash "$checker"
# Untracked paths do not change the index contract.
touch api/src/memory.rs
bash "$checker"
git add api/src/memory.rs
if bash "$checker" 2>errors; then
    echo 'missed staged source conflict' >&2; exit 1
fi
rg -q 'api/src/memory.rs' errors
git rm -q --cached api/src/memory.rs
bash "$checker"
# Nested guests, spaces and non-Rust directory children are included.
touch server/tests/target/cell/space.rs 'tools/with space.rs'
git add server tools
if bash "$checker" 2>errors; then
    echo 'missed nested guest conflict' >&2; exit 1
fi
rg -q 'server/tests/target/cell/space.rs' errors
rg -Fq 'tools/with\ space.rs' errors
echo 'layout checker contracts passed'
