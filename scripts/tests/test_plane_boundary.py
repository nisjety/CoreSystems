import importlib.util
import pathlib
import unittest

SPEC = importlib.util.spec_from_file_location(
    'plane_boundary', pathlib.Path(__file__).resolve().parents[1] / 'gates' / 'plane-boundary.py'
)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


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


if __name__ == '__main__':
    unittest.main()
