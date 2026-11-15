#!/usr/bin/env bash
set -euo pipefail
tag_only=false
if [[ ${1:-} == --tag-only ]]; then tag_only=true; shift; fi
tag=${1:?release tag required}
pattern='^([0-9]{2})\.([0-9]{2})\.([0-9]{2})-((explore|develop|preview)-([A-Z][a-z]+)|[A-Z][A-Za-z0-9]*)$'
if [[ ! $tag =~ $pattern ]]; then
    echo "invalid release tag: $tag" >&2; exit 1
fi
year=${BASH_REMATCH[1]} month=${BASH_REMATCH[2]} day=${BASH_REMATCH[3]}
channel=${BASH_REMATCH[5]} greek=${BASH_REMATCH[6]}
# Match the original strptime %y interpretation, including its 1969 pivot.
if (( 10#$year <= 68 )); then year=20$year; else year=19$year; fi
if [[ $(date -u --date="$year-$month-$day" +%Y.%m.%d 2>/dev/null) != "$year.$month.$day" ]]; then
    echo "invalid date in release tag: $tag" >&2; exit 1
fi
if [[ -n $channel ]]; then
    case $greek in
        Alpha|Beta|Gamma|Delta|Epsilon|Zeta|Eta|Theta|Iota|Kappa|Lambda|Mu|Nu|Xi|Omicron|Pi|Rho|Sigma|Tau|Upsilon|Phi|Chi|Psi|Omega) ;;
        *) echo "invalid Greek release suffix: $tag" >&2; exit 1 ;;
    esac
fi
if $tag_only; then exit 0; fi
commit=${2:?release commit required}
if git merge-base --is-ancestor "$commit" origin/main; then exit 0; fi
while IFS= read -r branch; do
    if git merge-base --is-ancestor "$commit" "$branch"; then exit 0; fi
done < <(git for-each-ref --format='%(refname)' 'refs/remotes/origin/release/*')
echo 'release tag must point to main or an applicable release/<codename> branch' >&2
exit 1
