# Preview publication

The first preview selects default ABI/Ledger 10 and Capsule format 2. The
[notes template](preview-alpha.md) is rendered by the release workflow using the
actual tag and exact Git SHA after its checks pass. The release preparation PR
records the exact candidate, locked validation and immutable consumer pins.

Before creating any tag, establish active protection against updates and deletion
for `refs/tags/*-preview-*`, with no bypass actors. An administrator can apply the
[prepared ruleset](preview-tags.ruleset.json):

```sh
gh api --method POST repos/shuyra-arche/kernel/rulesets --input docs/releases/preview-tags.ruleset.json
gh api repos/shuyra-arche/kernel/rulesets
# Fetch the returned ruleset ID and verify target=tag, enforcement=active,
# the include pattern, no bypass, and update/deletion rules before tagging.
gh api repos/shuyra-arche/kernel/rulesets/RULESET_ID
```

The current application connection returned HTTP 403 for the mutation. Do not
create an unprotected tag or treat candidate checks as publication. Once
protection is verified, use the actual publication date for
`YY.MM.DD-preview-Alpha`, check that the tag is unused, and point it at the
validated main revision. Keep it fixed. The existing Release workflow validates
ancestry and reruns checks before publishing a GitHub prerelease; no Cargo
registry packages are published. The workflow uses the committed notes template
for Preview Alpha and generated notes for other releases.
