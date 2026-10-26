#!/usr/bin/env python3
r"""Plane boundary gate: a plane reaches only its own databases and code.

Build plan WS12 S9. CLAUDE.md: no direct database crossing between planes, and a
plane does not build from another plane's tree. The gate reads the tracked files
under apps/<Plane>/ and applies three rules:

R1  A Postgres DSN in a docker-compose*.yml or a .env.example names a host that
    plane owns (its db_hosts) or a local host. A host no plane declares fails too.
R2  Non-test source has no cross-database link (dblink, postgres_fdw, CREATE
    SERVER, \connect), and no DSN naming a host another plane owns or a shared host.
R3  A manifest (Cargo.toml path, go.mod replace, package.json file: or link:)
    does not resolve into another plane's directory.

scripts/gates/plane-boundary.json holds each plane's database hosts, the shared
hosts and the exceptions. An exception is pinned to one file and one host or
target and carries a reason. One that covers nothing fails the gate, so it is
removed in the change that removes the reference. A plane missing from the file
owns no database host.

`--base-ref REF` also fails a change that grants more than the base did, read from
REF: a database or manifest exception that is new, a host added to a plane's
db_hosts, a plane newly listed, a new shared or local host, or a rule loosened (a
source suffix dropped, a test directory added, a cross-database pattern removed).
Removals and reason-text edits are not compared. A base with no policy file yet, the
change that introduces the gate, is not compared.

Usage, from the repository root:
    python3 scripts/gates/plane-boundary.py --check [--base-ref origin/<branch>]
    python3 scripts/gates/plane-boundary.py --self-test
Exit codes: 0 ok, 1 a boundary violation, a stale exception or a relaxation, 2 the gate cannot run.
"""
import ast
import argparse
import json
import posixpath
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CONFIG_REL = 'scripts/gates/plane-boundary.json'
SCRIPT_REL = 'scripts/gates/plane-boundary.py'
FIXTURES_REL = 'scripts/tests/fixtures/plane-boundary'
PLANES_DIR = 'apps'
POLICY_KEYS = {'local_hosts', 'planes', 'shared_hosts', 'db_exceptions', 'manifest_exceptions'}
COMPOSE = re.compile(r'(^|/)docker-compose[^/]*\.ya?ml$')
ENV_EXAMPLE = '.env.example'
SOURCE_SUFFIXES = ('.rs', '.go', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.py', '.sh', '.sql')
TEST_DIRS = {'test', 'tests', '__tests__', 'testdata', 'fixtures', 'e2e'}
TEST_NAME = re.compile(r'(^test_|_test\.|\.test\.|\.spec\.)')
DSN = re.compile(r'(?i)\bpostgres(?:ql)?(?:\+[a-z0-9_]+)?://')
DSN_KEY = re.compile(r'(?i)(DATABASE_URL|DSN)\s*[:=]')
KEYWORD_HOST = re.compile(r'(?i)\bhost\s*=\s*[\'"]?([A-Za-z0-9_.-]+)')
AUTHORITY_END = re.compile(r'[/?#\s\'"`<>(),\]]')
PLACEHOLDER = re.compile(r'\$\{([A-Za-z_][A-Za-z0-9_]*)(?:(:?[-=?])([^{}]*))?\}')
CROSS_DB_LINKS = (
    ('dblink', re.compile(r'(?i)\bdblink')),
    ('postgres_fdw', re.compile(r'(?i)\bpostgres_fdw')),
    ('CREATE SERVER', re.compile(r'(?i)\bcreate\s+server\b')),
    ('\\connect', re.compile(r'\\connect\b')),
)
MANIFEST_TARGETS = {
    'Cargo.toml': re.compile(r'\bpath\s*=\s*"([^"]+)"'),
    'go.mod': re.compile(r'=>\s*"?(\.\.?/[^"\r\n]*?)"?(?=\s*(?://|$))', re.MULTILINE),
    'package.json': re.compile(r'"(?:file|link):([^"]+)"'),
}
# The rule constants a change may loosen, read from the base branch by --base-ref.
RULE_NAMES = ('SOURCE_SUFFIXES', 'TEST_DIRS', 'CROSS_DB_LINKS')


def git(root, *args):
    return subprocess.run(['git', '-C', str(root), *args], stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL, check=True).stdout


def tracked_files(root):
    return [name for name in git(root, 'ls-files', '-z').decode('utf-8').split('\0') if name]


def read(path):
    return path.read_text(encoding='utf-8', errors='replace')


def _resolve(match):
    name, op, default = match.group(1), match.group(2), match.group(3)
    return default if op in (':-', '-', ':=', '=') else name


def expand(line):
    """Resolve ${VAR:-default} to its default and any other placeholder to its name, innermost first."""
    while True:
        expanded = PLACEHOLDER.sub(_resolve, line)
        if expanded == line:
            return line
        line = expanded


def dsn_hosts(line, keyword_form=False):
    """The hosts that the Postgres DSNs on a line name, with placeholders resolved."""
    text = expand(line)
    hosts = []
    for match in DSN.finditer(text):
        authority = AUTHORITY_END.split(text[match.end():], maxsplit=1)[0]
        host = authority.rsplit('@', 1)[-1]
        host = host[1:].split(']', 1)[0] if host.startswith('[') else host.split(':', 1)[0]
        if host:
            hosts.append(host)
    if keyword_form and DSN_KEY.search(text):
        hosts.extend(KEYWORD_HOST.findall(text))
    return hosts


def plane_of(rel):
    parts = rel.split('/')
    return parts[1] if len(parts) >= 3 and parts[0] == PLANES_DIR else None


def is_test(rel):
    parts = rel.split('/')
    return any(part in TEST_DIRS for part in parts[:-1]) or bool(TEST_NAME.search(parts[-1]))


def expect_keys(obj, keys, where):
    if not isinstance(obj, dict) or set(obj) != keys:
        raise ValueError(f'{where} must have exactly the keys {sorted(keys)}')


class Policy:
    """scripts/gates/plane-boundary.json, validated."""

    def __init__(self, data):
        expect_keys(data, POLICY_KEYS, 'the policy')
        self.local = set(data['local_hosts'])
        self.planes = {}
        for name, entry in data['planes'].items():
            expect_keys(entry, {'db_hosts'}, f'planes[{name!r}]')
            self.planes[name] = set(entry['db_hosts'])
        self.shared = dict(data['shared_hosts'])
        clash = sorted(host for host in self.shared if self.owners(host))
        if clash:
            raise ValueError(f'{", ".join(clash)} is both a shared host and a plane database host')
        self.db_exceptions = self._exceptions(data['db_exceptions'], 'host')
        self.manifest_exceptions = self._exceptions(data['manifest_exceptions'], 'target')

    @staticmethod
    def _exceptions(entries, key):
        found = {}
        for entry in entries:
            expect_keys(entry, {'file', key, 'reason'}, f'an exception ({key})')
            if not str(entry['reason']).strip():
                raise ValueError(f'the exception for {entry["file"]} needs a reason')
            found[(entry['file'], entry[key])] = entry['reason']
        return found

    def owners(self, host):
        return sorted(name for name, hosts in self.planes.items() if host in hosts)


def load_policy(root):
    return Policy(json.loads((root / CONFIG_REL).read_text(encoding='utf-8')))


def describe(policy, plane, host):
    owners = policy.owners(host)
    if owners:
        return f"it belongs to {', '.join(owners)}, and a plane may not reach another plane's database"
    if host in policy.shared:
        return 'it is a shared host that no plane may use'
    return f"if {plane} runs that store, list it under planes[{plane!r}].db_hosts in {CONFIG_REL}"


def check_database_file(rel, plane, text, policy, used, problems):
    allowed = policy.planes.get(plane, set()) | policy.local
    for lineno, line in enumerate(text.splitlines(), 1):
        for host in dsn_hosts(line, keyword_form=True):
            if host in allowed:
                continue
            if (rel, host) in policy.db_exceptions:
                used.add((rel, host))
                continue
            problems.append(f'{rel}:{lineno}: DSN host {host!r} is not a database host of {plane}: '
                            f'{describe(policy, plane, host)}')


def check_source(rel, plane, text, policy, used, problems):
    own = policy.planes.get(plane, set()) | policy.local
    for lineno, line in enumerate(text.splitlines(), 1):
        for label, pattern in CROSS_DB_LINKS:
            if pattern.search(line):
                problems.append(f'{rel}:{lineno}: {label} is a cross-database link; '
                                'source may not reach another database')
        for host in dsn_hosts(line):
            if host in own or not (policy.owners(host) or host in policy.shared):
                continue
            if (rel, host) in policy.db_exceptions:
                used.add((rel, host))
                continue
            problems.append(f'{rel}:{lineno}: DSN host {host!r} in source: {describe(policy, plane, host)}')


def check_manifest(rel, plane, text, policy, used, problems):
    pattern = MANIFEST_TARGETS[posixpath.basename(rel)]
    for match in pattern.finditer(text):
        target = posixpath.normpath(posixpath.join(posixpath.dirname(rel), match.group(1)))
        target_plane = plane_of(target)
        if target_plane is None or target_plane == plane:
            continue
        if (rel, target) in policy.manifest_exceptions:
            used.add((rel, target))
            continue
        lineno = text.count('\n', 0, match.start()) + 1
        problems.append(f"{rel}:{lineno}: points into {target_plane} at {target}; "
                        "a plane may not build from another plane's tree")


def kind_of(rel):
    """The rule a tracked file is checked against: database, manifest, source, or None."""
    name = posixpath.basename(rel)
    if COMPOSE.search(rel) or name == ENV_EXAMPLE:
        return 'database'
    if name in MANIFEST_TARGETS:
        return 'manifest'
    if rel.endswith(SOURCE_SUFFIXES) and not is_test(rel):
        return 'source'
    return None


def check(root):
    """The boundary problems in the tracked tree at root, with the counts the success line reports."""
    policy = load_policy(root)
    problems, used_db, used_manifest = [], set(), set()
    counts = {'database': 0, 'source': 0, 'manifest': 0}
    for rel in tracked_files(root):
        plane = plane_of(rel)
        kind = kind_of(rel) if plane else None
        if kind is None:
            continue
        path = root / rel
        if path.is_symlink() or not path.is_file():
            continue
        counts[kind] += 1
        text = read(path)
        if kind == 'database':
            check_database_file(rel, plane, text, policy, used_db, problems)
        elif kind == 'source':
            check_source(rel, plane, text, policy, used_db, problems)
        else:
            check_manifest(rel, plane, text, policy, used_manifest, problems)
    for rel, host in sorted(policy.db_exceptions):
        if (rel, host) not in used_db:
            problems.append(f'stale database exception: {host!r} in {rel} covers nothing now; '
                            f'remove its entry from {CONFIG_REL}')
    for rel, target in sorted(policy.manifest_exceptions):
        if (rel, target) not in used_manifest:
            problems.append(f'stale manifest exception: {target} from {rel} covers nothing now; '
                            f'remove its entry from {CONFIG_REL}')
    return problems, counts, len(used_db), len(used_manifest)


def policy_at(root, ref):
    """The policy file as it stands at a git ref, through the same validator as the working tree."""
    return Policy(json.loads(git(root, 'show', f'{ref}:{CONFIG_REL}').decode('utf-8')))


def cross_db_links(node):
    """The (label, pattern) pairs of a CROSS_DB_LINKS assignment, read from its AST without running the script."""
    if not isinstance(node, ast.Tuple):
        raise ValueError(f'{SCRIPT_REL} must set CROSS_DB_LINKS as a tuple of (label, re.compile(pattern)) pairs')
    links = set()
    for entry in node.elts:
        well_formed = (isinstance(entry, ast.Tuple) and len(entry.elts) == 2 and isinstance(entry.elts[1], ast.Call)
                       and isinstance(entry.elts[1].func, ast.Attribute) and entry.elts[1].func.attr == 'compile'
                       and len(entry.elts[1].args) == 1 and not entry.elts[1].keywords)
        if not well_formed:
            raise ValueError(f'{SCRIPT_REL} must set each CROSS_DB_LINKS entry as (label, re.compile(pattern))')
        label, call = entry.elts
        links.add((ast.literal_eval(label), ast.literal_eval(call.args[0])))
    return links


def rules_at(root, ref):
    """The rule constants the gate script sets at a git ref, read without running that script."""
    found = {}
    for node in ast.parse(git(root, 'show', f'{ref}:{SCRIPT_REL}').decode('utf-8')).body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            if node.targets[0].id in RULE_NAMES:
                found[node.targets[0].id] = node.value
    if set(found) != set(RULE_NAMES):
        raise ValueError(f'{SCRIPT_REL} at {ref} must set {", ".join(RULE_NAMES)}')
    return {
        'SOURCE_SUFFIXES': set(ast.literal_eval(found['SOURCE_SUFFIXES'])),
        'TEST_DIRS': set(ast.literal_eval(found['TEST_DIRS'])),
        'CROSS_DB_LINKS': cross_db_links(found['CROSS_DB_LINKS']),
    }


def current_rules():
    """The rule constants as this script stands, in the shape rules_at returns."""
    return {
        'SOURCE_SUFFIXES': set(SOURCE_SUFFIXES),
        'TEST_DIRS': set(TEST_DIRS),
        'CROSS_DB_LINKS': {(label, pattern.pattern) for label, pattern in CROSS_DB_LINKS},
    }


def loosened_rules(base, head):
    """Rule changes that let more source through than the base did."""
    notes = []
    for suffix in sorted(base['SOURCE_SUFFIXES'] - head['SOURCE_SUFFIXES']):
        notes.append(f'SOURCE_SUFFIXES no longer counts {suffix!r}')
    for name in sorted(head['TEST_DIRS'] - base['TEST_DIRS']):
        notes.append(f'TEST_DIRS gains {name!r}')
    for label, pattern in sorted(base['CROSS_DB_LINKS'] - head['CROSS_DB_LINKS']):
        notes.append(f'CROSS_DB_LINKS loses the {label} pattern {pattern!r}')
    return notes


def relaxations(base, head, base_ref):
    """The policy additions the change makes that the policy at base_ref does not already grant."""
    grant = f'a relaxation of the policy at {base_ref}'
    notes = []
    for rel, host in sorted(set(head.db_exceptions) - set(base.db_exceptions)):
        notes.append(f'{CONFIG_REL}: adds database exception {host!r} for {rel}, {grant}')
    for rel, target in sorted(set(head.manifest_exceptions) - set(base.manifest_exceptions)):
        notes.append(f'{CONFIG_REL}: adds manifest exception {target} for {rel}, {grant}')
    for name in sorted(set(head.planes) - set(base.planes)):
        notes.append(f'{CONFIG_REL}: lists plane {name!r}, {grant}')
    for name in sorted(head.planes):
        for host in sorted(head.planes[name] - base.planes.get(name, set())):
            notes.append(f'{CONFIG_REL}: adds {host!r} to planes[{name!r}].db_hosts, {grant}')
    for host in sorted(set(head.shared) - set(base.shared)):
        notes.append(f'{CONFIG_REL}: adds shared host {host!r}, {grant}')
    for host in sorted(head.local - base.local):
        notes.append(f'{CONFIG_REL}: adds local host {host!r}, {grant}')
    return notes


def compare_with_base(root, base_ref):
    """What the change grants beyond the policy and rules at base_ref; a base with no policy file is not compared."""
    git(root, 'rev-parse', '--verify', '--quiet', f'{base_ref}^{{commit}}')
    present = subprocess.run(['git', '-C', str(root), 'cat-file', '-e', f'{base_ref}:{CONFIG_REL}'],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0
    if not present:
        print(f'note: {base_ref} has no {CONFIG_REL} yet, so this change is not compared with it')
        return []
    notes = relaxations(policy_at(root, base_ref), load_policy(root), base_ref)
    for note in loosened_rules(rules_at(root, base_ref), current_rules()):
        notes.append(f'{SCRIPT_REL}: {note}, a relaxation of the rules at {base_ref}')
    return notes


def self_test(root):
    """Each fixture runs as a throwaway repository and must match its expect.json; returns the mismatches."""
    failures = []
    for case in sorted(path for path in (root / FIXTURES_REL).iterdir() if path.is_dir()):
        expect = json.loads((case / 'expect.json').read_text(encoding='utf-8'))
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
            repo = Path(tmp)
            shutil.copytree(case, repo, dirs_exist_ok=True)
            git(repo, 'init', '-q')
            git(repo, '-c', 'core.excludesFile=', 'add', '-A')
            problems = check(repo)[0]
        text = '\n'.join(problems)
        code = 1 if problems else 0
        wanted = expect.get('contains', [])
        missing = [needle for needle in wanted if needle not in text]
        if code != expect['exit'] or missing:
            detail = f'; missing {missing}' if missing else ''
            failures.append(f'fixture {case.name}: expected exit {expect["exit"]} with {wanted}, '
                            f'got exit {code}{detail}' + ''.join(f'\n    {p}' for p in problems))
    return failures


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true', help='fail on a boundary violation or a stale exception (default)')
    mode.add_argument('--self-test', action='store_true', help='run the fixtures under scripts/tests/fixtures/plane-boundary')
    ap.add_argument('--base-ref', help='also fail if this change grants more than the policy and rules at this git ref')
    ap.add_argument('--root', type=Path, default=REPO_ROOT, help=argparse.SUPPRESS)
    args = ap.parse_args(argv)

    try:
        if args.self_test:
            failures = self_test(args.root)
            if failures:
                for line in failures:
                    print(f'::error::{line}')
                return 1
            print('plane boundary self-test ok: every fixture matches its expect.json')
            return 0
        problems, counts, db_used, manifest_used = check(args.root)
        if args.base_ref is not None:
            problems += compare_with_base(args.root, args.base_ref)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.CalledProcessError) as exc:
        print(f'::error::plane boundary cannot run: {exc}')
        return 2

    if problems:
        print(f'::error::plane boundary: {len(problems)} problem(s)')
        for line in problems[:50]:
            print(f'  {line}')
        if len(problems) > 50:
            print(f'  ... and {len(problems) - 50} more')
        print(f'move the reference to the plane that owns it, or record a reviewed exception in {CONFIG_REL}')
        return 1
    print(f'plane boundary ok: {counts["database"]} database files, {counts["source"]} source files, '
          f'{counts["manifest"]} manifests; {db_used} database and {manifest_used} manifest exceptions in use')
    return 0


if __name__ == '__main__':
    sys.exit(main())
