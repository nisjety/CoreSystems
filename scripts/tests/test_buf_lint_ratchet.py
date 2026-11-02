import importlib.util
import json
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'buf_lint_ratchet', pathlib.Path(__file__).resolve().parents[1] / 'gates' / 'buf-lint-ratchet.py'
)
ratchet = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ratchet)


def row(path, typ, msg, line=1):
    return json.dumps({'path': path, 'start_line': line, 'type': typ, 'message': msg})


class RatchetTest(unittest.TestCase):
    def setUp(self):
        self.dir = pathlib.Path(tempfile.mkdtemp())
        self.baseline = self.dir / 'baseline.json'
        self.lint = self.dir / 'lint.json'

    def write_lint(self, *rows):
        self.lint.write_text('\n'.join(rows) + '\n', encoding='utf-8')

    def test_unchanged_set_passes_even_when_lines_move(self):
        self.write_lint(row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment', 10))
        self.assertEqual(ratchet.main(['--baseline', str(self.baseline), '--update', str(self.lint)]), 0)
        self.write_lint(row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment', 99))
        self.assertEqual(ratchet.main(['--baseline', str(self.baseline), str(self.lint)]), 0)

    def test_a_new_finding_fails(self):
        self.write_lint(row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment'))
        ratchet.main(['--baseline', str(self.baseline), '--update', str(self.lint)])
        self.write_lint(
            row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment'),
            row('a.proto', 'FIELD_LOWER_SNAKE_CASE', 'Field "Y" should be lower_snake_case'),
        )
        self.assertEqual(ratchet.main(['--baseline', str(self.baseline), str(self.lint)]), 1)

    def test_a_stale_entry_is_reported_but_passes(self):
        self.write_lint(
            row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment'),
            row('b.proto', 'COMMENT_FIELD', 'Field "z" needs a comment'),
        )
        ratchet.main(['--baseline', str(self.baseline), '--update', str(self.lint)])
        self.write_lint(row('a.proto', 'COMMENT_FIELD', 'Field "x" needs a comment'))
        self.assertEqual(ratchet.main(['--baseline', str(self.baseline), str(self.lint)]), 0)


if __name__ == '__main__':
    unittest.main()
