import contextlib
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'log_content', pathlib.Path(__file__).resolve().parents[1] / 'gates' / 'log-content.py'
)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]


def findings_of(src):
    return gate.scan_file('probe.rs', src)[0]


def reasons_of(src):
    return [f['reasons'] for f in findings_of(src)]


class RuleTest(unittest.TestCase):
    def test_a_prompt_field_is_flagged(self):
        # The acceptance case: a new log line that carries the prompt must go red.
        self.assertEqual(reasons_of('fn f(p: &str) { tracing::info!(prompt = %p); }'), [['name:prompt']])

    def test_content_words_inside_message_text_are_not_flagged(self):
        src = 'fn f() { tracing::debug!(%request_id, "deep research synthesis returned empty content"); }'
        self.assertEqual(findings_of(src), [])

    def test_longer_identifiers_are_not_the_denylisted_name(self):
        self.assertEqual(findings_of('fn f(n: usize) { info!(text_len = n, body_bytes = n); }'), [])

    def test_inline_capture_of_a_content_name_is_flagged(self):
        self.assertEqual(reasons_of('fn f(text: &str) { info!("got {text}"); }'), [['capture:text']])

    def test_inline_capture_of_a_longer_name_is_not_flagged(self):
        self.assertEqual(findings_of('fn f(n: usize) { info!("got {text_len}"); }'), [])

    def test_sigil_on_a_whole_request_is_flagged(self):
        self.assertEqual(reasons_of('fn f(req: R) { tracing::warn!(?req, "bad"); }'), [['sigil:req']])
        self.assertEqual(reasons_of('fn f() { info!(?self.request, "x"); }'), [['sigil:request']])

    def test_a_field_of_a_request_is_not_the_request(self):
        src = 'fn f() { info!(org_id = %req.org_id, request_id = %req.request_id, "x"); }'
        self.assertEqual(findings_of(src), [])

    def test_debug_format_of_a_request_argument_is_flagged(self):
        self.assertEqual(reasons_of('fn f(req: R) { debug!("request {:?}", req); }'), [['debug-arg:req']])

    def test_debug_format_of_a_request_field_is_not_flagged(self):
        self.assertEqual(findings_of('fn f(req: R) { debug!("status {:?}", req.method); }'), [])

    def test_a_request_used_as_a_field_name_is_not_flagged(self):
        self.assertEqual(findings_of('fn f(id: u8) { info!(request = %id, "x"); }'), [])

    def test_a_display_sigil_on_a_body_is_flagged_by_name(self):
        self.assertEqual(reasons_of('fn f(body: &str) { warn!(%body, "x"); }'), [['name:body']])

    def test_macro_rules_definitions_are_not_invocations(self):
        self.assertEqual(findings_of('macro_rules! info { ($($t:tt)*) => {} }'), [])


class LexerTest(unittest.TestCase):
    def test_a_multiline_macro_is_flagged_at_its_first_line(self):
        src = 'fn f() {\n    info!(\n        msg = "a ) b",\n        x = g(h(1)),\n        body = %body,\n    );\n}'
        self.assertEqual([(f['line'], f['reasons']) for f in findings_of(src)], [(2, ['name:body'])])

    def test_raw_strings_and_char_literals_do_not_unbalance_the_macro(self):
        src = (
            'fn f() { info!(x = r#"prompt"#, y = \'{\', z = \'\\\'\'); '
            'warn!(prompt = %p); }'
        )
        self.assertEqual(reasons_of(src), [['name:prompt']])

    def test_lifetimes_are_not_char_literals(self):
        self.assertEqual(reasons_of("fn f<'a>(x: &'a str) { info!(prompt = %x); }"), [['name:prompt']])

    def test_comments_inside_macros_are_ignored(self):
        src = 'fn f(p: &str) { info!(// prompt\n    /* text */ n = 1); }'
        self.assertEqual(findings_of(src), [])

    def test_an_unbalanced_invocation_is_reported_as_a_gate_error(self):
        with self.assertRaises(gate.GateError):
            gate.scan_file('probe.rs', 'fn f(p: &str) { info!(prompt = %p; }')


class AllowAndHashTest(unittest.TestCase):
    def test_an_allow_comment_above_the_macro_suppresses_it(self):
        src = 'fn f(p: &str) {\n    // log-content: allow(logs a length, not the prompt)\n    info!(prompt = %p);\n}'
        findings, allowed = gate.scan_file('probe.rs', src)
        self.assertEqual((findings, allowed), ([], 1))

    def test_an_allow_comment_inside_the_macro_suppresses_it(self):
        src = 'fn f(p: &str) { info!(\n    // log-content: allow(count only)\n    prompt = p.len()); }'
        findings, allowed = gate.scan_file('probe.rs', src)
        self.assertEqual((findings, allowed), ([], 1))

    def test_an_allow_comment_needs_a_reason(self):
        src = 'fn f(p: &str) {\n    // log-content: allow()\n    info!(prompt = %p);\n}'
        self.assertEqual(reasons_of(src), [['name:prompt']])
        src = 'fn f(p: &str) {\n    // log-content: allow(())\n    info!(prompt = %p);\n}'
        self.assertEqual(reasons_of(src), [['name:prompt']])

    def test_the_hash_ignores_layout_and_comments(self):
        compact = 'fn f(p: &str) { info!(prompt = %p, n = 1); }'
        spread = 'fn f(p: &str) {\n    info!(\n        // the prompt is the subject\n        prompt = %p,\n        n = 1,\n    );\n}'
        self.assertEqual(findings_of(compact)[0]['sha256'], findings_of(spread)[0]['sha256'])

    def test_the_hash_changes_when_the_macro_changes(self):
        a = findings_of('fn f(p: &str) { info!(prompt = %p, n = 1); }')[0]
        b = findings_of('fn f(p: &str) { info!(prompt = %p, n = 2); }')[0]
        self.assertNotEqual(a['sha256'], b['sha256'])


class BaselineTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        for rel in gate.SCOPE:
            (self.root / rel).mkdir(parents=True)
        self.src = self.root / gate.SCOPE[0] / 'lib.rs'
        self.src.write_text('fn f(text: &str) {\n    warn!(bytes = text.len(), "too big");\n}\n', encoding='utf-8')
        self.baseline = self.root / 'baseline.json'

    def tearDown(self):
        self.tmp.cleanup()

    def run_gate(self, *args):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            code = gate.main(['--root', str(self.root), '--baseline', str(self.baseline), *args])
        return code, out.getvalue()

    def set_review(self, text):
        doc = json.loads(self.baseline.read_text(encoding='utf-8'))
        for entry in doc['findings']:
            entry['review'] = text
        self.baseline.write_text(json.dumps(doc, indent=1), encoding='utf-8')

    def test_an_unreviewed_baseline_entry_fails_until_it_has_a_reason(self):
        self.assertEqual(self.run_gate('--update')[0], 0)
        self.assertEqual(self.run_gate('--check')[0], 1)
        self.set_review('logs a byte count, not the text')
        self.assertEqual(self.run_gate('--check')[0], 0)

    def test_a_new_prompt_log_is_red(self):
        self.run_gate('--update')
        self.set_review('logs a byte count, not the text')
        self.src.write_text(self.src.read_text(encoding='utf-8') + 'fn g(p: &str) { tracing::info!(prompt = %p); }\n',
                            encoding='utf-8')
        code, out = self.run_gate('--check')
        self.assertEqual(code, 1)
        self.assertIn('name:prompt', out)

    def test_update_keeps_existing_reviews(self):
        self.run_gate('--update')
        self.set_review('logs a byte count, not the text')
        self.assertEqual(self.run_gate('--update')[0], 0)
        self.assertEqual(self.run_gate('--check')[0], 0)

    def test_a_stale_entry_is_reported_and_passes(self):
        self.run_gate('--update')
        self.set_review('logs a byte count, not the text')
        self.src.write_text('fn f() {}\n', encoding='utf-8')
        code, out = self.run_gate('--check')
        self.assertEqual(code, 0)
        self.assertIn('no longer occur', out)

    def test_a_missing_scope_directory_is_a_gate_error(self):
        (self.root / gate.SCOPE[1]).rmdir()
        self.assertEqual(self.run_gate('--check')[0], 2)

    def test_a_missing_baseline_is_a_gate_error(self):
        self.assertEqual(self.run_gate('--check')[0], 2)


class CommittedBaselineTest(unittest.TestCase):
    def test_every_committed_baseline_entry_has_a_review(self):
        path = REPO_ROOT / 'scripts' / 'gates' / 'baselines' / 'log-content.json'
        doc = json.loads(path.read_text(encoding='utf-8'))
        self.assertEqual(doc['count'], len(doc['findings']))
        unreviewed = [e['path'] for e in doc['findings'] if not e.get('review')]
        self.assertEqual(unreviewed, [])


if __name__ == '__main__':
    unittest.main()
