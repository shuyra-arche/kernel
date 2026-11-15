#!/usr/bin/env bash
set -euo pipefail
declare -A run=([native]=false [smp]=false [activity]=false [flat]=false [room]=false [system]=false [iommu]=false)
all() { for key in "${!run[@]}"; do run[$key]=true; done; }
if [[ ${EVENT:-} != pull_request ]]; then
    all
else
    : "${BASE:?PR base required}" "${HEAD:?PR head required}"
    while IFS= read -r -d '' path; do
        case $path in
            api/*|client/*|server/src/*|server/Cargo.toml|server/build.rs|Cargo.*|.cargo/*|.github/workflows/check.yml|server/tests/runner/*|tools/verification-scope.sh) all ;;
            server/tests/target/basis/flat/*) run[flat]=true ;;
            server/tests/target/basis/nest/memo/activity/*) run[activity]=true ;;
            server/tests/target/basis/room/*) run[room]=true ;;
            server/tests/target/metal/riscv/mmu/completion/*) run[smp]=true ;;
            server/tests/target/metal/riscv/iommu/*|server/tests/metal/riscv/iommu/*|server/tests/support/iommu/*) run[iommu]=true ;;
        esac
    done < <(git diff --name-only -z "$BASE" "$HEAD")
fi
guest=false
for key in native smp activity flat room system iommu; do
    echo "$key=${run[$key]}" >> "${GITHUB_OUTPUT:?output file required}"
    if [[ ${run[$key]} == true ]]; then guest=true; fi
done
echo "guest=$guest" >> "$GITHUB_OUTPUT"
