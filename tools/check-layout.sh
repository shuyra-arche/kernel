#!/usr/bin/env bash
# Check the Git index, including nested standalone guests and staged additions.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
declare -A sources=() directories=()
while IFS= read -r -d '' path; do
    [[ $path != *.rs ]] || sources[${path%.rs}]=1
    parent=$path
    while [[ $parent == */* ]]; do
        parent=${parent%/*}
        directories[$parent]=1
    done
done < <(git ls-files --cached --deduplicate -z)
status=0
for stem in "${!sources[@]}"; do
    if [[ ${directories[$stem]+present} ]]; then
        printf 'Rust file/directory conflict: %q + %q\n' "$stem.rs" "$stem/" >&2
        status=1
    fi
done
exit "$status"
