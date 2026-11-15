#!/usr/bin/env bash
set -euo pipefail
selector=$(cd "$(dirname "$0")/.." && pwd)/verification-scope.sh
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cd "$scratch"
git init -q
git -c user.name=Test -c user.email=test@example.com commit --allow-empty -qm root
export EVENT=pull_request BASE
BASE=$(git rev-parse HEAD)
export GITHUB_OUTPUT=$scratch/output
case_change() {
    mkdir -p "$(dirname "$1")"
    touch "$1"
    git add .
    git -c user.name=Test -c user.email=test@example.com commit -qm scenario
    export HEAD
    HEAD=$(git rev-parse HEAD)
    : > "$GITHUB_OUTPUT"
    bash "$selector"
}
case_change server/tests/basis/name/mod.rs
grep -qx 'guest=false' "$GITHUB_OUTPUT"
BASE=$HEAD
case_change server/tests/target/basis/flat/main.rs
grep -qx 'flat=true' "$GITHUB_OUTPUT"
grep -qx 'native=false' "$GITHUB_OUTPUT"
BASE=$HEAD
case_change server/src/basis/flat/mod.rs
if grep -q '=false' "$GITHUB_OUTPUT"; then echo 'production change skipped a scenario' >&2; exit 1; fi
HEAD=missing
if bash "$selector" 2>/dev/null; then echo 'invalid diff did not fail closed' >&2; exit 1; fi
echo 'verification scope contracts passed'
