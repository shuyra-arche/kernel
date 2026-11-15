#!/usr/bin/env python3
"""Refresh the review catalogue; this textual index is not a lock-safety proof."""
import json
import re
from pathlib import Path
ROOT = Path(__file__).resolve().parents[3]
SYNC = ROOT / 'server/src/metal/riscv/sync'
ORDER = (SYNC / 'order/mod.rs').read_text()
classes = re.findall(r'^    (\w+),$', ORDER[ORDER.index('pub enum Class {'):ORDER.index('impl Class {')], re.M)
relations = {a: re.findall(r'\w+', b) for a, b in re.findall(r'^    (\w+) => \[([^\n]*)\],', ORDER, re.M)}
reach = {a: set(relations.get(a, [])) for a in classes}
# Report the minimal displayed edges of the declared closure, not a second rule.
reduced = {a: sorted(b for b in reach[a] if not any(b in reach[c] for c in reach[a] if c != b)) for a in classes}
fields = {a: [] for a in classes}
calls = []
for path in sorted((ROOT / 'server/src').rglob('*.rs')):
    source = path.read_text()
    relative = str(path.relative_to(ROOT))
    for match in re.finditer(r'(?:LegacyLock|Lock)\s*<\s*(?:crate::metal::sync::)?order::(\w+)', source):
        name = match[1]
        if name in fields:
            line = source.count('\n', 0, match.start()) + 1
            text = source.splitlines()[line - 1].strip()
            fields[name].append({'path': relative, 'line': line, 'declaration': text})
    # Include owner methods returning raw guards, static globals and multiline
    # calls. The source context is authoritative where names are overloaded.
    functions = list(re.finditer(r'\bfn\s+(\w+)', source))
    for match in re.finditer(r'\.(lock|try_lock|with|pair|try_pair|reenter|legacy_lock|legacy_try_lock|try_source_borrow)\s*\(', source):
        before = source[:match.start()]
        owner = next((m[1] for m in reversed(functions) if m.start() < match.start()), '<static>')
        line = before.count('\n') + 1
        calls.append({'path': relative, 'line': line, 'enclosing_function': owner, 'operation': match[1], 'source': source.splitlines()[line - 1].strip()})
# OnceLock/type aliases and fully qualified declarations are indexed above;
# additionally retain their whole source navigation for zero-use leaf classes.
report = {'scope': 'textual current declarations and call sites; see README.md for ownership and lifetime evidence', 'classes': [{'class': a, 'typed_declarations': fields[a], 'minimal_declared_predecessors': [b for b in classes if a in reduced[b]], 'minimal_declared_successors': reduced[a]} for a in classes], 'acquisition_sites': calls}
(ROOT / 'docs/synchronization/acquisitions.json').write_text(json.dumps(report, indent=2) + '\n')
lines = ['# Current lock fields and acquisition relations', '', 'Generated with `python3 server/tests/sync/inventory.py`. The [audit](README.md) supplies owner, lifetime, failure, IRQ and migration obligations. The [complete call-site catalogue](acquisitions.json) links each textual acquisition to its source function; overloaded names and `with` calls require the source context. This is a review index, not execution coverage.', '', 'Only this table’s predecessors/successors are a display of the transitive reduction of the production `relations!` closure. They are permitted directions, not claims that every possible edge is executed. All omitted ordinary combinations are forbidden; controlled peers and source-borrow try admission are separate protocols.', '', '| Class | Current typed storage/navigation | Minimal declared predecessors | Minimal declared successors |', '| --- | --- | --- | --- |']
for a in classes:
    refs = ', '.join(f'[{x["path"].removeprefix("server/src/")}:{x["line"]}](../../{x["path"]}#L{x["line"]})' for x in fields[a]) or ('injected guest probe only' if a == 'ProbeRegistry' else 'see constructor migration table and owner audit')
    lines.append(f'| `{a}` | {refs} | '+', '.join(f'`{b}`' for b in classes if a in reduced[b])+' | '+', '.join(f'`{b}`' for b in reduced[a])+' |')
(ROOT / 'docs/synchronization/fields.md').write_text('\n'.join(lines)+'\n')
print(f'{len(classes)} classes; {len(calls)} textual call sites; {sum(map(len, fields.values()))} concrete type references')
