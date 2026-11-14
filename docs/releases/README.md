# Preview publication

The first preview selects default ABI/Ledger 10 and Capsule format 2. The
[notes template](preview-alpha.md) is rendered by the release workflow using the
actual tag and exact Git SHA after its checks pass. The release preparation PR
records the exact candidate, locked validation and immutable consumer pins.

## Published source preview

[26.11.14-preview-Alpha](https://github.com/shuyra-arche/kernel/releases/tag/26.11.14-preview-Alpha)
was published on 2026-11-14 at 17:45 Asia/Shanghai. Its fixed source revision is
`bc1976a61158d972e3b550cf6699c571e6deebc7`. It is a GitHub prerelease, with
source archives and release notes; no Cargo registry packages or complete
Vanllia/System product are published.

The [release workflow](https://github.com/shuyra-arche/kernel/actions/runs/43581620935)
passed all checks before publication: locked host tests, RV64 debug/release,
ABI/native selections and all six QEMU shards. The publication build used
`rustc 1.102.0-nightly (5f4effd57 2026-11-13)`; pre-tag candidate and component
checks used the earlier `8eb778d04` nightly (Twine Rust 1.99.0). The preparation and exact
consumer input records are in [#165](https://github.com/shuyra-arche/kernel/pull/165).
Per-page A/D stays Unknown/Unsupported, ABI selection depends on the recorded release revision, and full System
migration and physical-hardware qualification remain outside this preview.

The preview-matching ruleset 27257248 was verified before Tag creation with
update/deletion restrictions and no bypass. After publication, the administrator
replaced it with [ruleset 27258030](https://github.com/shuyra-arche/kernel/rules/27258030),
which is Active and protects the complete date-based namespace below, including
all three non-stable channels and stable codenames. Update/deletion restrictions
and no bypass were verified again. Keep the published Tag fixed.

## Protection and future publication

Before creating any tag, establish active protection against updates and deletion
for every date-based release Tag, with no bypass actors. This covers all three
non-stable channels (`explore`, `develop`, `preview`) and stable codenames.

In the GitHub web UI's Tag pattern input, enter only:

```text
[0-9][0-9].[0-9][0-9].[0-9][0-9]-*
```

The API represents the same pattern as
`refs/tags/[0-9][0-9].[0-9][0-9].[0-9][0-9]-*`. The web UI adds `refs/tags/`
automatically; including it in that input produces a duplicate prefix and does
not match real release refs. The pattern protects the release-name namespace;
the release workflow separately validates calendar dates, channels and suffixes.

An administrator can apply the [prepared ruleset](preview-tags.ruleset.json)
through the API:

```sh
gh api --method POST repos/shuyra-arche/kernel/rulesets --input docs/releases/preview-tags.ruleset.json
# For an existing ruleset, PUT its ID instead of creating a second ruleset:
# gh api --method PUT repos/shuyra-arche/kernel/rulesets/RULESET_ID --input docs/releases/preview-tags.ruleset.json
gh api repos/shuyra-arche/kernel/rulesets
# Fetch the returned ruleset ID and verify target=tag, enforcement=active,
# the include pattern, no bypass, and update/deletion rules before tagging.
gh api repos/shuyra-arche/kernel/rulesets/RULESET_ID
```

The application connection returned HTTP 403 for ruleset administration; an
administrator enabled the matching protection for the first preview. Do not
create an unprotected release Tag. For future releases, verify the matching
scope, choose an unused actual-date Tag following the shared release standard,
and point it at the validated main revision. Keep published Tags fixed. The existing Release workflow validates
ancestry and reruns checks before publishing a GitHub prerelease; no Cargo
registry packages are published. The workflow uses the committed notes template
for Preview Alpha and generated notes for other releases.
