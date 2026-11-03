import contextlib
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'pin_actions', pathlib.Path(__file__).resolve().parents[1] / 'gates' / 'pin-actions.py'
)
pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pin)

SHA = '3d3c42e5aac5ba805825da76410c181273ba90b1'
DIGEST = 'sha256:' + '0' * 64
WORKFLOW = '.github/workflows/ci.yml'


def step(uses):
    return f'jobs:\n  build:\n    steps:\n      - uses: {uses}\n'


class PinActionsTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / '.github' / 'workflows').mkdir(parents=True)

    def write(self, body, rel=WORKFLOW):
        file = self.root / rel
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(body, encoding='utf-8')

    def write_baseline(self, entries):
        file = self.root / 'baseline.json'
        file.write_text(json.dumps(entries), encoding='utf-8')
        return str(file)

    def gate(self, *extra):
        with contextlib.redirect_stdout(io.StringIO()):
            return pin.main(['--check', '--root', str(self.root), *extra])

    def test_sha_pinned_with_tag_comment_passes(self):
        self.write(step(f'actions/checkout@{SHA} # v7'))
        self.assertEqual(self.gate(), 0)

    def test_tag_ref_fails(self):
        self.write(step('actions/checkout@v7'))
        self.assertEqual(self.gate(), 1)

    def test_branch_ref_fails(self):
        self.write(step('dtolnay/rust-toolchain@stable'))
        self.assertEqual(self.gate(), 1)

    def test_missing_ref_fails(self):
        self.write(step('actions/checkout'))
        self.assertEqual(self.gate(), 1)

    def test_sha_without_tag_comment_fails(self):
        self.write(step(f'actions/checkout@{SHA}'))
        self.assertEqual(self.gate(), 1)

    def test_blank_comment_fails(self):
        self.write(step(f'actions/checkout@{SHA} #'))
        self.assertEqual(self.gate(), 1)

    def test_local_actions_are_exempt(self):
        self.write(step('./.github/actions/setup'))
        self.assertEqual(self.gate(), 0)

    def test_reusable_workflow_ref_is_checked(self):
        self.write(step('owner/repo/.github/workflows/build.yml@v1'))
        self.assertEqual(self.gate(), 1)
        self.write(step(f'owner/repo/.github/workflows/build.yml@{SHA} # v1'))
        self.assertEqual(self.gate(), 0)

    def test_docker_image_needs_a_digest(self):
        self.write(step('docker://alpine:3.19 # 3.19'))
        self.assertEqual(self.gate(), 1)
        self.write(step(f'docker://alpine@{DIGEST} # 3.19'))
        self.assertEqual(self.gate(), 0)

    def test_quoted_values_are_read(self):
        self.write(step("'actions/checkout@v7'"))
        self.assertEqual(self.gate(), 1)
        self.write(step(f"'actions/checkout@{SHA}' # v7"))
        self.assertEqual(self.gate(), 0)

    def test_flow_style_uses_is_checked(self):
        self.write('jobs:\n  build:\n    steps: [{uses: actions/checkout@v7}]\n')
        self.assertEqual(self.gate(), 1)

    def test_commented_out_uses_is_ignored(self):
        self.write('# uses: actions/checkout@v7\n')
        self.assertEqual(self.gate(), 0)

    def test_composite_actions_are_scanned(self):
        self.write(step('actions/checkout@v7'), rel='.github/actions/setup/action.yml')
        self.assertEqual(self.gate(), 1)

    def test_no_workflow_files_cannot_run(self):
        self.assertEqual(self.gate(), 2)

    def test_baseline_tolerates_a_listed_violation(self):
        self.write(step('actions/checkout@v7'))
        baseline = self.write_baseline([{'file': WORKFLOW, 'uses': 'actions/checkout@v7'}])
        self.assertEqual(self.gate('--baseline', baseline), 0)

    def test_baseline_tolerates_only_the_listed_occurrences(self):
        self.write('jobs:\n  build:\n    steps:\n'
                   '      - uses: actions/checkout@v7\n'
                   '      - uses: actions/checkout@v7\n')
        baseline = self.write_baseline([{'file': WORKFLOW, 'uses': 'actions/checkout@v7'}])
        self.assertEqual(self.gate('--baseline', baseline), 1)

    def test_violation_outside_the_baseline_fails(self):
        self.write(step('actions/checkout@v7'))
        baseline = self.write_baseline([])
        self.assertEqual(self.gate('--baseline', baseline), 1)

    def test_stale_baseline_entry_fails(self):
        self.write(step(f'actions/checkout@{SHA} # v7'))
        baseline = self.write_baseline([{'file': WORKFLOW, 'uses': 'actions/checkout@v7'}])
        self.assertEqual(self.gate('--baseline', baseline), 1)

    def test_malformed_baseline_cannot_run(self):
        self.write(step(f'actions/checkout@{SHA} # v7'))
        baseline = self.write_baseline({'file': WORKFLOW})
        self.assertEqual(self.gate('--baseline', baseline), 2)

    def test_missing_baseline_cannot_run(self):
        self.write(step(f'actions/checkout@{SHA} # v7'))
        self.assertEqual(self.gate('--baseline', str(self.root / 'absent.json')), 2)

    def test_check_flag_is_required(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
            pin.main(['--root', str(self.root)])
        self.assertEqual(raised.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
