#!/usr/bin/env python3
"""Ratchet for `buf lint`: fail only on findings that are not in the baseline.

`buf lint "apps/Model Plane/proto"` reports about 1,500 pre-existing findings
(mostly missing comments). Failing on all of them would block every proto
change; ignoring the output would let new violations in silently. This gate
keys each finding by (path, type, message) - deliberately NOT by line number,
so unrelated edits that shift lines do not churn the baseline - and fails on
any key that the committed baseline does not contain.

Usage:
    buf lint "apps/Model Plane/proto" --error-format=json > /tmp/lint.json || true
    python3 scripts/gates/buf-lint-ratchet.py --baseline <baseline.json> /tmp/lint.json
    python3 scripts/gates/buf-lint-ratchet.py --baseline <baseline.json> --update /tmp/lint.json

The baseline can only shrink in review: `--update` rewrites it from the current
findings (a deliberate, reviewed step). Entries that no longer occur are
reported as stale so the baseline can be tightened.
"""
import argparse
import json
import sys


def load_findings(path):
    keys = set()
    with open(path, encoding='utf-8') as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            keys.add((row['path'], row['type'], row['message']))
    return keys


def load_baseline(path):
    with open(path, encoding='utf-8') as fh:
        data = json.load(fh)
    return {(e['path'], e['type'], e['message']) for e in data['findings']}


def write_baseline(path, keys):
    findings = [{'path': p, 'type': t, 'message': m} for p, t, m in sorted(keys)]
    with open(path, 'w', encoding='utf-8', newline='\n') as fh:
        json.dump({'version': 1, 'count': len(findings), 'findings': findings}, fh, indent=1, sort_keys=True)
        fh.write('\n')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--baseline', required=True)
    ap.add_argument('--update', action='store_true', help='rewrite the baseline from the current findings')
    ap.add_argument('lint_json', help='output of `buf lint --error-format=json` (one JSON object per line)')
    args = ap.parse_args(argv)

    current = load_findings(args.lint_json)
    if args.update:
        write_baseline(args.baseline, current)
        print(f'baseline rewritten: {len(current)} findings')
        return 0

    baseline = load_baseline(args.baseline)
    new = sorted(current - baseline)
    stale = sorted(baseline - current)
    if stale:
        print(f'note: {len(stale)} baseline entries no longer occur; refresh with --update in a reviewed PR')
    if new:
        print(f'::error::buf lint: {len(new)} finding(s) not in the baseline')
        for p, t, m in new[:50]:
            print(f'  {p}: {t}: {m}')
        if len(new) > 50:
            print(f'  ... and {len(new) - 50} more')
        return 1
    print(f'buf lint ratchet ok: {len(current)} findings, none new ({len(baseline)} in baseline)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
