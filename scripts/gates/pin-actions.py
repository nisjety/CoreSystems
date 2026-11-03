#!/usr/bin/env python3
"""Pinned GitHub Actions: every repository action a workflow runs names a commit SHA.

EP-9 C6 (compliance plan 062). A tag or branch such as `actions/checkout@v7` is moved by
its owner, so a workflow that passed yesterday can run different code today. A `uses:`
value is pinned when its ref is a full 40-hex commit SHA and a trailing comment names the
tag it was resolved from (`uses: owner/repo@<sha> # v7`): the comment keeps the version
readable, and Dependabot reads it to propose the next SHA. A `docker://` image is pinned
by its `@sha256:` digest. Local actions (`./...`) live in this repository and are exempt.

The gate reads files only, so it runs offline with no token. It finds a `uses` key in
block style (`- uses: x`) and in flow style (`{uses: x}`), and it reports a value it
cannot read rather than skipping it.

`--baseline` is a ratchet: a JSON list of `{"file": ..., "uses": ...}` entries for
violations tolerated for now. A violation outside the list fails, and so does a listed
entry that no longer matches, so each fix must also remove its entry. Adding an entry is
a change to the baseline file, reviewed like any other. No baseline is committed because
every action is pinned today.

Usage, from the repository root:
    python3 scripts/gates/pin-actions.py --check [--baseline <file.json>]
Exit codes: 0 ok, 1 an unpinned action, a missing tag comment or a stale baseline entry,
2 the gate cannot run.
"""
import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
# Composite actions carry their own `uses:` steps, so they are checked with the workflows.
SCAN_GLOBS = (
    '.github/workflows/*.yml',
    '.github/workflows/*.yaml',
    '.github/actions/**/*.yml',
    '.github/actions/**/*.yaml',
)
# A value is quoted, or runs to a space, comma or brace, so a `#` glued to it stays part of
# the ref, as YAML reads it. An empty value still matches, so a `uses:` with nothing after
# it is reported instead of skipped.
USES_KEY = re.compile(r"""(?:^|[\s{,-])uses\s*:\s*("[^"]*"|'[^']*'|[^\s"',}]*)""")
COMMENT = re.compile(r'#\s*\S')
SHA = re.compile(r'[0-9a-f]{40}')
DOCKER_DIGEST = re.compile(r'@sha256:[0-9a-f]{64}$')


def pin_problem(value, has_comment):
    """Why a `uses` value is not pinned, or None when it is local or pinned with its tag named."""
    if value.startswith('./'):
        return None
    if value.startswith('docker://'):
        pinned = DOCKER_DIGEST.search(value) is not None
        wanted = 'an @sha256 image digest'
    else:
        _, at, ref = value.rpartition('@')
        pinned = bool(at) and SHA.fullmatch(ref) is not None
        wanted = 'a 40-hex commit SHA'
    if not pinned:
        return f'{value} is not pinned to {wanted}'
    if not has_comment:
        return f'{value} has no trailing comment naming its tag'
    return None


def workflow_files(root):
    return sorted({path for pattern in SCAN_GLOBS for path in root.glob(pattern) if path.is_file()})


def scan_file(path, rel):
    """(file, line, uses, reason) for each `uses` value in one file that is not pinned."""
    found = []
    for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
        if line.lstrip().startswith('#'):
            continue
        match = USES_KEY.search(line)
        if match is None:
            continue
        value = match.group(1).strip('"\'')
        reason = pin_problem(value, COMMENT.search(line, match.end()) is not None)
        if reason is not None:
            found.append((rel, number, value, reason))
    return found


def valid_entry(entry):
    return isinstance(entry, dict) and set(entry) == {'file', 'uses'} and all(
        isinstance(text, str) for text in entry.values())


def load_baseline(path):
    if path is None:
        return []
    entries = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(entries, list) or not all(valid_entry(entry) for entry in entries):
        raise ValueError('the baseline must be a JSON list of {"file": ..., "uses": ...} objects')
    return entries


def compare(found, baseline):
    """Problems for each violation the baseline does not tolerate and each entry nothing matches."""
    tolerated = Counter((entry['file'], entry['uses']) for entry in baseline)
    problems = []
    for rel, number, value, reason in found:
        if tolerated[(rel, value)] > 0:
            tolerated[(rel, value)] -= 1
        else:
            problems.append(f'{rel}:{number}: {reason}')
    for (rel, value), left in sorted(tolerated.items()):
        problems.extend(f'{rel}: baseline entry {value} matches nothing now; remove it' for _ in range(left))
    return problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--check', action='store_true',
                    help='fail on an unpinned action, a missing tag comment or a stale baseline entry')
    ap.add_argument('--baseline', type=Path,
                    help='JSON list of tolerated {"file", "uses"} entries; each fix removes its entry')
    ap.add_argument('--root', type=Path, default=REPO_ROOT, help=argparse.SUPPRESS)
    args = ap.parse_args(argv)
    if not args.check:
        ap.error('nothing to do: pass --check')

    root = args.root
    try:
        files = workflow_files(root)
        if not files:
            print('::error::pin actions cannot run: no workflow or composite action files found')
            return 2
        found = [item for path in files for item in scan_file(path, path.relative_to(root).as_posix())]
        baseline = load_baseline(args.baseline)
    except (OSError, ValueError) as exc:
        print(f'::error::pin actions cannot run: {exc}')
        return 2

    problems = compare(found, baseline)
    if problems:
        print(f'::error::pin actions: {len(problems)} problem(s)')
        for line in problems[:50]:
            print(f'  {line}')
        if len(problems) > 50:
            print(f'  ... and {len(problems) - 50} more')
        print('pin each action to a commit SHA with its tag in a trailing comment: uses: owner/repo@<sha> # <tag>')
        return 1
    print(f'pin actions ok: {len(files)} files, every uses: is pinned, local or baselined')
    return 0


if __name__ == '__main__':
    sys.exit(main())
