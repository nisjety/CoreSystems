"""Tests for the SSE taxonomy gate (apps/Model Plane/scripts/gen-sse-event-taxonomy.py).

A gate is only worth its CI minutes if it can fail. Each case below edits one real
source text in memory and asserts the gate's verdict. The live tree is the positive
control: it must build, and each mutation must be the thing that turns it red.
"""

import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
GATE_PATH = ROOT / 'apps' / 'Model Plane' / 'scripts' / 'gen-sse-event-taxonomy.py'
SPEC = importlib.util.spec_from_file_location('gen_sse_event_taxonomy', GATE_PATH)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)


def once(text, old, new):
    """Replace the only occurrence of `old`. A probe anchor that is not unique proves nothing."""
    found = text.count(old)
    if found != 1:
        raise AssertionError(f'probe anchor must occur exactly once, found {found}: {old!r}')
    return text.replace(old, new, 1)


class TaxonomyGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rust = gate.load_rust()
        cls.proto = gate.read(gate.ORCHESTRATION_PROTO)
        cls.consumer = gate.read(gate.CONSUMER)

    def build(self, rust=None, proto=None, consumer=None):
        return gate.build(
            self.rust if rust is None else rust,
            self.proto if proto is None else proto,
            self.consumer if consumer is None else consumer,
        )

    def edit_rust(self, name, old, new):
        rust = dict(self.rust)
        rust[name] = once(self.rust[name], old, new)
        return rust

    def test_live_tree_builds_and_lists_every_other_producer(self):
        taxonomy = self.build()
        self.assertIn('## Other producers', taxonomy.doc)
        self.assertIn('| `run_parked` | parked agentic run', taxonomy.doc)
        self.assertIn('| `awaiting_approval` | agentic run paused', taxonomy.doc)
        self.assertIn('| `plan_transitioned` | 10 | ✅ |', taxonomy.doc)

    def test_an_spa_case_that_no_producer_writes_is_red(self):
        consumer = once(
            self.consumer,
            "    case 'connected':",
            "    case 'mystery_event':\n    case 'connected':",
        )
        with self.assertRaisesRegex(gate.TaxonomyError, "'mystery_event'"):
            self.build(consumer=consumer)

    def test_a_renamed_orchestration_arm_is_red(self):
        renamed_wire = self.edit_rust(
            'sse.rs',
            'orchestration_event::Event::RunResumed(_) => "run_resumed",',
            'orchestration_event::Event::RunResumed(_) => "run_resumed_v2",',
        )
        with self.assertRaisesRegex(gate.TaxonomyError, "'run_resumed_v2'"):
            self.build(rust=renamed_wire)

        renamed_field = once(
            self.proto, 'RunResumed run_resumed = 25;', 'RunResumed resumed_run = 25;'
        )
        with self.assertRaisesRegex(gate.TaxonomyError, 'names no OrchestrationEvent member'):
            self.build(proto=renamed_field)

    def test_comments_inside_the_arm_lists_change_nothing(self):
        rust = dict(self.rust)
        rust['sse.rs'] = once(
            self.rust['sse.rs'],
            '        orchestration_event::Event::RunParked(_) => "run_parked",',
            '        // Ids only: the park reason is never broadcast.\n'
            '        orchestration_event::Event::RunParked(_) => "run_parked",',
        )
        rust['sse_events.rs'] = once(
            self.rust['sse_events.rs'],
            '            ChatEvent::Citation { .. }\n            | ChatEvent::Grounding { .. }',
            '            ChatEvent::Citation { .. }\n'
            '            // Grounding is read with the citations it anchors.\n'
            '            | ChatEvent::Grounding { .. }',
        )
        self.assertEqual(self.build(rust=rust).doc, self.build().doc)

    def test_only_the_native_switch_counts_as_handled(self):
        # The AG-UI CUSTOM switch still says `case 'artifact':`. The native path must not
        # inherit that label, or deleting the native case would go unnoticed.
        consumer = once(
            self.consumer,
            "\n    case 'artifact':\n      handlers.onArtifact",
            "\n    case 'artifact_renamed':\n      handlers.onArtifact",
        )
        with self.assertRaisesRegex(gate.TaxonomyError, "'artifact'"):
            self.build(consumer=consumer)

    def test_an_unlisted_literal_writer_is_red_and_test_code_is_not(self):
        production_writer = (
            '\nfn stray_frame() -> Event {\n    Event::default().event("mystery_frame")\n}\n'
        )
        with self.assertRaisesRegex(gate.TaxonomyError, "'mystery_frame'"):
            self.build(rust=self.append_rust('sse.rs', production_writer))

        test_writer = (
            '\n#[cfg(test)]\nmod probe_tests {\n    fn writes() -> Event {\n'
            '        Event::default().event("test_only_frame")\n    }\n}\n'
        )
        taxonomy = self.build(rust=self.append_rust('sse.rs', test_writer))
        self.assertNotIn('test_only_frame', taxonomy.doc)

    def append_rust(self, name, text):
        rust = dict(self.rust)
        rust[name] = self.rust[name] + text
        return rust

    def test_a_listed_literal_writer_that_is_gone_is_red(self):
        rust = self.edit_rust('sse.rs', '.event("awaiting_approval")', '.event(AWAITING_APPROVAL)')
        with self.assertRaisesRegex(gate.TaxonomyError, "'awaiting_approval'"):
            self.build(rust=rust)

    def test_a_declared_member_with_no_arm_is_listed_as_not_written(self):
        proto = once(
            self.proto,
            'RunResumed run_resumed = 25;',
            'RunResumed run_resumed = 25;\n    ProbeEvent probe_event = 40;',
        )
        self.assertIn('| `probe_event` | 40 | not yet |', self.build(proto=proto).doc)

    def test_a_oneof_field_this_parser_cannot_read_fails_loudly(self):
        proto = once(
            self.proto,
            'RunResumed run_resumed = 25;',
            'RunResumed run_resumed = 25 [deprecated = true];',
        )
        with self.assertRaisesRegex(gate.TaxonomyError, 'but the oneof assigns'):
            self.build(proto=proto)

    def test_test_items_are_removed_whole_and_production_is_kept(self):
        source = (
            'fn keep() {}\n'
            '#[cfg(test)]\n'
            'mod tests {\n'
            '    fn helper(\n'
            '        a: u8,\n'
            '    ) -> u8 {\n'
            '        if a > 0 { 1 } else { 0 }\n'
            '    }\n'
            '}\n'
            '#[cfg(test)]\n'
            'use super::{keep, Other};\n'
            'fn after() {}\n'
        )
        self.assertEqual(gate.without_test_items(source), 'fn keep() {}\nfn after() {}\n')

    def test_a_test_item_the_parser_does_not_understand_fails_loudly(self):
        with self.assertRaisesRegex(gate.TaxonomyError, 'unsupported'):
            gate.without_test_items('#[cfg(test)]\nimpl Probe for Thing\n')


if __name__ == '__main__':
    unittest.main()
