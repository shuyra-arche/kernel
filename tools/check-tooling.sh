#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
status=0
while IFS= read -r -d '' path; do
    case $path in
        *.py) printf 'retired interpreter source: %q\n' "$path" >&2; status=1 ;;
    esac
done < <(git ls-files --cached -z)
if rg -n 'python[0-9]?([[:space:]]|$)|/run\.py|/prepare\.py' .github/workflows README.md server/tests/README.md; then
    echo 'interpreter dependency in verification entry points' >&2; status=1
fi
exit "$status"
