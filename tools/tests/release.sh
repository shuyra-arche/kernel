#!/usr/bin/env bash
set -euo pipefail
validator=$(cd "$(dirname "$0")/.." && pwd)/release.sh
for tag in 26.11.15-preview-Alpha 24.02.29-Orion 00.02.29-develop-Omega 69.01.01-Orion 26.11.15-X1; do
    bash "$validator" --tag-only "$tag"
done
for tag in 26.02.29-Orion 26.11.31-Orion 26.00.15-Orion 26.13.15-Orion 26.11.00-Orion 26.11.15-preview-Orion 26.11.15-Preview-Alpha 26.11.15-orion '26.11.15-Orion extra' '26.11.15-Orion$(id)'; do
    if bash "$validator" --tag-only "$tag" 2>/dev/null; then
        echo "accepted invalid tag: $tag" >&2; exit 1
    fi
done
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cd "$scratch"
git init -q
git -c user.name=Test -c user.email=test@example.com commit --allow-empty -qm root
root=$(git rev-parse HEAD)
git update-ref refs/remotes/origin/main "$root"
bash "$validator" 26.11.15-Orion "$root"
git -c user.name=Test -c user.email=test@example.com commit --allow-empty -qm unrelated
tip=$(git rev-parse HEAD)
if bash "$validator" 26.11.15-Orion "$tip" 2>/dev/null; then
    echo 'accepted commit outside published branches' >&2; exit 1
fi
git update-ref refs/remotes/origin/release/orion "$tip"
bash "$validator" 26.11.15-Orion "$tip"
echo 'release validation contracts passed'
