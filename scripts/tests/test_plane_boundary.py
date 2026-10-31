import contextlib
import importlib.util
import io
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    'plane_boundary', pathlib.Path(__file__).resolve().parents[1] / 'gates' / 'plane-boundary.py'
)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

GATE_SOURCE = pathlib.Path(gate.__file__).read_text(encoding='utf-8')


class ParsingTest(unittest.TestCase):
    def test_placeholders_resolve_to_their_default_or_their_name(self):
        self.assertEqual(gate.expand('${A:-${B:-plane-db}}'), 'plane-db')
        self.assertEqual(gate.expand('${PW:?PW is required}'), 'PW')
        self.assertEqual(gate.expand('${HOST}'), 'HOST')

    def test_the_dsn_host_is_read_past_placeholders_and_userinfo(self):
        line = ('DATABASE_URL: postgresql://${DB_USER:-model}:${DB_PASSWORD:?DB_PASSWORD is required}'
                '@${DB_HOST:-postgres}:5432/session_core')
        self.assertEqual(gate.dsn_hosts(line), ['postgres'])

    def test_port_and_ipv6_forms(self):
        self.assertEqual(gate.dsn_hosts('postgres://u@localhost:5433/x'), ['localhost'])
        self.assertEqual(gate.dsn_hosts('postgres://u@[::1]:5432/x'), ['::1'])

    def test_keyword_dsns_count_only_on_dsn_keys(self):
        self.assertEqual(
            gate.dsn_hosts('DATABASE_URL: host=controlplane-postgres dbname=x', keyword_form=True),
            ['controlplane-postgres'])
        self.assertEqual(gate.dsn_hosts('DB_HOST=controlplane-postgres', keyword_form=True), [])
        self.assertEqual(gate.dsn_hosts('DATABASE_URL: host=controlplane-postgres'), [])

    def test_test_source_is_recognised_by_directory_or_name(self):
        self.assertTrue(gate.is_test('apps/Control Plane/org-core/internal/config/config_test.go'))
        self.assertTrue(gate.is_test('apps/Frontend Plane/verevonv3/src/shared/x.test.ts'))
        self.assertTrue(gate.is_test('apps/Model Plane/scripts/tests/gate.sh'))
        self.assertFalse(gate.is_test('apps/Model Plane/rust/services/execution-core/src/scrub.rs'))


class PolicyTest(unittest.TestCase):
    def policy(self, **changes):
        data = {'local_hosts': [], 'planes': {}, 'shared_hosts': {}, 'db_exceptions': [], 'manifest_exceptions': []}
        data.update(changes)
        return gate.Policy(data)

    def test_an_exception_needs_a_reason(self):
        with self.assertRaises(ValueError):
            self.policy(db_exceptions=[{'file': 'apps/a/x.yml', 'host': 'h', 'reason': ' '}])

    def test_a_host_cannot_be_both_shared_and_owned(self):
        with self.assertRaises(ValueError):
            self.policy(planes={'Model Plane': {'db_hosts': ['h']}}, shared_hosts={'h': 'shared'})

    def test_unknown_keys_are_rejected(self):
        with self.assertRaises(ValueError):
            gate.Policy({'local_hosts': [], 'planes': {}, 'shared_hosts': {}, 'db_exceptions': [],
                         'manifest_exceptions': [], 'extra': 1})


class SelfTest(unittest.TestCase):
    def test_every_fixture_matches_its_expectation(self):
        self.assertEqual(gate.self_test(gate.REPO_ROOT), [])


def policy_text(**changes):
    """A policy file's text: an empty policy with the given keys replaced."""
    data = {'local_hosts': [], 'planes': {}, 'shared_hosts': {}, 'db_exceptions': [], 'manifest_exceptions': []}
    data.update(changes)
    return json.dumps(data, indent=2) + '\n'


def compose_file(host):
    """A compose file whose only database reference is a DSN to host."""
    return f'services:\n  api:\n    environment:\n      DATABASE_URL: postgres://app@{host}:5432/app\n'


class Repo:
    """A throwaway git repository in which the gate runs with --root."""

    def __init__(self, testcase):
        tmp = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        testcase.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.git('init', '-q')

    def git(self, *args):
        result = subprocess.run(
            ['git', '-C', str(self.root), '-c', 'user.name=plane-boundary',
             '-c', 'user.email=plane-boundary@example.invalid', *args],
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        return result.stdout.decode('utf-8').strip()

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8', newline='\n')

    def install_gate(self):
        # A base commit must carry the gate script, because the rules are read from it.
        self.write(gate.SCRIPT_REL, GATE_SOURCE)

    def commit(self):
        self.git('add', '-A')
        self.git('commit', '-q', '--allow-empty', '-m', 'change')
        return self.git('rev-parse', 'HEAD')

    def run(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = gate.main(['--root', str(self.root), *argv])
        return code, out.getvalue()


class BaseRefTest(unittest.TestCase):
    """--base-ref fails a change that grants more than the policy and rules at the base."""

    COMPOSE = 'apps/Model Plane/docker-compose.yml'
    MODEL = {'Model Plane': {'db_hosts': ['mp-postgres']}}

    def repo(self, files, **policy):
        """A repository whose base commit carries the gate, a policy and the given files, and that base commit."""
        repo = Repo(self)
        repo.install_gate()
        repo.write(gate.CONFIG_REL, policy_text(**policy))
        for rel, text in files.items():
            repo.write(rel, text)
        return repo, repo.commit()

    def test_a_new_database_exception_fails_against_the_base(self):
        repo, base = self.repo({self.COMPOSE: compose_file('mp-postgres')}, planes=self.MODEL)
        repo.write(self.COMPOSE, compose_file('controlplane-postgres'))
        repo.write(gate.CONFIG_REL, policy_text(planes=self.MODEL, db_exceptions=[
            {'file': self.COMPOSE, 'host': 'controlplane-postgres', 'reason': 'legacy link'}]))
        repo.commit()
        self.assertEqual(repo.run('--check')[0], 0)
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn("adds database exception 'controlplane-postgres'", out)

    def test_adding_a_host_to_a_plane_fails_against_the_base(self):
        repo, base = self.repo({self.COMPOSE: compose_file('mp-postgres')}, planes=self.MODEL)
        repo.write(self.COMPOSE, compose_file('controlplane-postgres'))
        repo.write(gate.CONFIG_REL, policy_text(
            planes={'Model Plane': {'db_hosts': ['mp-postgres', 'controlplane-postgres']}}))
        repo.commit()
        self.assertEqual(repo.run('--check')[0], 0)
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn("adds 'controlplane-postgres' to planes['Model Plane'].db_hosts", out)

    def test_removing_an_exception_passes_against_the_base(self):
        exception = {'file': self.COMPOSE, 'host': 'controlplane-postgres', 'reason': 'legacy link'}
        repo, base = self.repo({self.COMPOSE: compose_file('controlplane-postgres')},
                               planes=self.MODEL, db_exceptions=[exception])
        repo.write(self.COMPOSE, compose_file('mp-postgres'))
        repo.write(gate.CONFIG_REL, policy_text(planes=self.MODEL))
        repo.commit()
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 0)
        self.assertNotIn('relaxation', out)

    def test_a_reason_edit_is_not_a_relaxation(self):
        exception = {'file': self.COMPOSE, 'host': 'controlplane-postgres', 'reason': 'legacy link'}
        repo, base = self.repo({self.COMPOSE: compose_file('controlplane-postgres')},
                               planes=self.MODEL, db_exceptions=[exception])
        repo.write(gate.CONFIG_REL, policy_text(
            planes=self.MODEL, db_exceptions=[dict(exception, reason='retired in WS12')]))
        repo.commit()
        self.assertEqual(repo.run('--check', '--base-ref', base)[0], 0)

    def test_a_new_manifest_exception_fails_against_the_base(self):
        manifest = 'apps/Model Plane/rust/Cargo.toml'
        repo, base = self.repo({manifest: '[package]\nname = "model-core"\n'})
        repo.write(manifest, '[dependencies]\ncontrol-core = { path = "../../Control Plane/core" }\n')
        repo.write(gate.CONFIG_REL, policy_text(manifest_exceptions=[
            {'file': manifest, 'target': 'apps/Control Plane/core', 'reason': 'generated client'}]))
        repo.commit()
        self.assertEqual(repo.run('--check')[0], 0)
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn('adds manifest exception apps/Control Plane/core', out)

    def test_a_new_local_host_shared_host_or_plane_fails_against_the_base(self):
        cases = [
            ({'local_hosts': ['localhost', 'db.internal']}, "adds local host 'db.internal'"),
            ({'shared_hosts': {'nango-postgres': 'root external services'}}, "adds shared host 'nango-postgres'"),
            ({'planes': {'Support Plane': {'db_hosts': []}}}, "lists plane 'Support Plane'"),
        ]
        for change, wanted in cases:
            with self.subTest(wanted=wanted):
                repo, base = self.repo({})
                repo.write(gate.CONFIG_REL, policy_text(**change))
                repo.commit()
                code, out = repo.run('--check', '--base-ref', base)
                self.assertEqual(code, 1)
                self.assertIn(wanted, out)

    def test_a_dropped_source_suffix_fails_against_the_base(self):
        repo, base = self.repo({})
        suffixes = tuple(suffix for suffix in gate.SOURCE_SUFFIXES if suffix != '.sql')
        with mock.patch.object(gate, 'SOURCE_SUFFIXES', suffixes):
            code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn("SOURCE_SUFFIXES no longer counts '.sql'", out)

    def test_a_test_directory_added_fails_against_the_base(self):
        repo, base = self.repo({})
        with mock.patch.object(gate, 'TEST_DIRS', gate.TEST_DIRS | {'src'}):
            code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn("TEST_DIRS gains 'src'", out)

    def test_a_dropped_cross_database_pattern_fails_against_the_base(self):
        repo, base = self.repo({})
        links = tuple(link for link in gate.CROSS_DB_LINKS if link[0] != 'dblink')
        with mock.patch.object(gate, 'CROSS_DB_LINKS', links):
            code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 1)
        self.assertIn('CROSS_DB_LINKS loses the dblink pattern', out)

    def test_the_rules_read_from_the_base_script_match_the_running_script(self):
        repo, base = self.repo({})
        self.assertEqual(gate.rules_at(repo.root, base), gate.current_rules())

    def test_an_unchanged_policy_and_rules_pass_against_the_base(self):
        repo, base = self.repo({self.COMPOSE: compose_file('mp-postgres')}, planes=self.MODEL)
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 0)
        self.assertIn('plane boundary ok', out)

    def test_a_base_without_the_policy_file_is_not_compared(self):
        repo = Repo(self)
        repo.install_gate()
        base = repo.commit()
        repo.write(self.COMPOSE, compose_file('controlplane-postgres'))
        repo.write(gate.CONFIG_REL, policy_text(planes=self.MODEL, db_exceptions=[
            {'file': self.COMPOSE, 'host': 'controlplane-postgres', 'reason': 'legacy link'}]))
        repo.commit()
        code, out = repo.run('--check', '--base-ref', base)
        self.assertEqual(code, 0)
        self.assertIn('not compared', out)

    def test_an_unknown_base_ref_cannot_run(self):
        repo, _ = self.repo({})
        code, out = repo.run('--check', '--base-ref', 'origin/no-such-branch')
        self.assertEqual(code, 2)
        self.assertIn('cannot run', out)

    def test_a_base_policy_that_fails_the_validator_cannot_run(self):
        repo, base = self.repo({}, db_exceptions=[{'file': self.COMPOSE, 'host': 'h', 'reason': ' '}])
        repo.write(gate.CONFIG_REL, policy_text())
        repo.commit()
        self.assertEqual(repo.run('--check', '--base-ref', base)[0], 2)


if __name__ == '__main__':
    unittest.main()
