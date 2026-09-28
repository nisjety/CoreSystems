//! Opt-in source-review rule for labelled author proposals.
//!
//! `MODEL_GATEWAY_REVIEW_PROPOSAL_RULE` is `off` (the default) or `on`. On,
//! every source review (a single call, each batch of a split review and the
//! recheck) gets [`PROPOSAL_RULE`] right after its base instruction, and an
//! accepted receipt records [`PROPOSAL_RULE_ID`] as its `reviewRule`. Off,
//! every review request, receipt and reuse binding is byte for byte what it
//! was before the switch existed. Unset, empty or any other value is off, so a
//! typo never selects the unvalidated rule. The value is read once per process
//! ([`ReviewProposalRule::from_env`]) and carried on `AppState` and
//! `crate::source_validation::CheckSettings`; tests pass it and never mutate
//! the environment. The author, its private repair and the word repair never
//! receive the rule.
//!
//! The rule tells the reviewer how to judge text the author labels as its own
//! proposal. The recommendation itself has no truth value; it passes when it
//! is labelled, contradicts no source statement or constraint, and relies only
//! on source-supported facts, and then its segment is `supported` with the
//! source lines for those facts. Anything factual inside a proposal is judged
//! exactly as without the label. The review schema and its statuses do not
//! change.
//!
//! # The rule is not validated yet
//!
//! It is off until the component gates (the labelled review matrix and the
//! recheck controls, run with the switch off and on) and the journeys pass
//! with it, and the paired controls it adds carry labels the owner has not
//! confirmed yet (`tests/support/proposal_rule_controls.rs`). Until those
//! controls pass with confirmed labels, nothing here shows that the rule closes
//! any clause, invented dates and durations inside proposals included. Turning
//! it on changes the reviewer of every journey. The tests below check that
//! neither the rule nor those controls carry a token of the maintained
//! product-recording fixtures or an acceptance-oracle stem (a lexical test
//! cannot detect a paraphrase of the answer key), that every reject control
//! differs from its accept twin in one line, and that every control reaches
//! the reviewer, with the computed-schedule note or the request where it is
//! meant to test them, instead of being decided by a local guard.

/// Switches the rule: `off` or `on`.
pub const REVIEW_PROPOSAL_RULE_ENV: &str = "MODEL_GATEWAY_REVIEW_PROPOSAL_RULE";

/// The rule a source review gets when [`ReviewProposalRule::On`]. Any change
/// to this text needs a new [`PROPOSAL_RULE_ID`] (the pin test enforces it).
///
/// `proposal-rule-2` replaced `proposal-rule-1` before either was run live,
/// after a review of rule-1 found that: the label sentence contradicted the
/// accepted source-role proposals (a label may make a recommendation
/// acceptable, never a factual assertion inside it); "judged only on three
/// things" dropped the base check of segments against each other, so two
/// proposals could form a cycle; source text alone could open the exception
/// for roles the sources do not name, which now needs the request or a brief
/// the task adopts, and a request for each task's responsible role is not one;
/// "sets a decision deadline" also read on proposing that one step come before
/// another; a date range read as either a date or a duration (it is a
/// duration); and a lead-in line was not named with the heading.
pub const PROPOSAL_RULE: &str = "Author proposals: an author proposal is text that the candidate explicitly labels as its own proposal, suggestion or recommendation, such as 'Proposal:' or 'Suggestion:' in the document's language, in the same sentence, list item or table cell, or in the header of its table column. A heading, a lead-in line or a document-wide draft label makes nothing a proposal; a statement that someone else proposed something is a factual claim about the sources; and a value presented as calculated or derived is a claim about that calculation, whatever its label. Check every other assertion in the segment as usual. A labelled recommendation that asserts no new fact is judged only on three things: it is clearly labelled; it contradicts no source statement or constraint, waives, relaxes or works around no stated condition, and conflicts with no other segment, other proposals included, for example by forming a circular or impossible sequence with them; and every fact it relies on, such as a role, responsibility, dependency, condition or stated gap, follows from the sources, not merely from another segment. The recommendation itself has no truth value: do not try to disprove it or reject it only because no source recommends it. If it passes and the segment's other assertions are supported, mark the segment supported and cite the source lines for the facts it relies on and the constraints it respects; only a proposal that relies on no source fact stays no_assertions. A label never makes acceptable a factual assertion inside the proposal that would be unsupported without it. The segment is unsupported if a proposal names a person the sources do not name, or a role they do not name unless proposed roles are requested; gives anyone approval, sign-off or decision authority, or sets a date or time limit for a decision, that the sources do not give; uses a date, duration, number or quantity that the sources do not give and these rules do not otherwise allow; presupposes an approval, status, agreement, completed action, existing arrangement or available data that the sources do not establish; presents anything else as true without source support, including what the proposal will achieve; or carries a label that misstates how a value was obtained. Proposed roles are requested only when the user's request asks for proposed owners or roles, or when the task adopts a supplied brief that asks for them; with no request supplied, such a brief suffices. Asking for each task's responsible role does not by itself request proposed roles. Proposing only that one step come before another sets no time limit for a decision and is judged by the three criteria. A date range or a period set aside states a duration, which needs a duration the sources give even where a single later date may be chosen. Name the failing clause in the reason. Adding a proposal label never makes acceptable anything this paragraph rejects, and a suggested edit must not add a label to make a rejected claim pass. Document requirements still apply to proposals.";

/// Recorded on a receipt, and in its reuse binding, reviewed under
/// [`PROPOSAL_RULE`].
pub const PROPOSAL_RULE_ID: &str = "proposal-rule-2";

/// Whether source reviews get [`PROPOSAL_RULE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReviewProposalRule {
    /// Today's review, byte for byte. The default until the rule's live
    /// validation passes.
    #[default]
    Off,
    /// Every source review also gets [`PROPOSAL_RULE`]. Opt-in.
    On,
}

impl ReviewProposalRule {
    /// Both settings, off first.
    pub const ALL: [Self; 2] = [Self::Off, Self::On];

    /// The operator's setting: `off` or `on`, ignoring case and surrounding
    /// whitespace. Unset, empty or anything else is off, so a mistyped value
    /// (`1`, `true` and `yes` included) never selects the unvalidated rule.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        value.and_then(Self::recognise).unwrap_or_default()
    }

    fn recognise(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "on" => Some(Self::On),
            _ => None,
        }
    }

    /// The operator's setting, read once like
    /// `crate::latency_levers::LatencyLevers::from_env` and
    /// `crate::author_guidance::AuthorGuidance::from_env`.
    #[must_use]
    pub fn from_env() -> Self {
        static RULE: std::sync::OnceLock<ReviewProposalRule> = std::sync::OnceLock::new();
        *RULE.get_or_init(|| {
            let value = std::env::var(REVIEW_PROPOSAL_RULE_ENV).ok();
            let rule = Self::parse(value.as_deref());
            if value
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty() && Self::recognise(value).is_none())
            {
                tracing::warn!(
                    env = REVIEW_PROPOSAL_RULE_ENV,
                    "review proposal rule is neither off nor on; using off"
                );
            }
            tracing::info!(review_proposal_rule = rule.name(), "review proposal rule");
            rule
        })
    }

    /// The setting's name, as the variable spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }

    /// The paragraph a source review gets, if any.
    #[must_use]
    pub const fn instruction(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::On => Some(PROPOSAL_RULE),
        }
    }

    /// What an accepted receipt and the reuse binding record; `None` keeps
    /// today's bytes.
    #[must_use]
    pub const fn receipt_id(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::On => Some(PROPOSAL_RULE_ID),
        }
    }
}

/// The paired review controls, shared with the live harnesses, so the guard
/// below checks exactly the texts those harnesses send.
#[cfg(test)]
#[path = "../tests/support/proposal_rule_controls.rs"]
mod controls;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author_guidance::leakage;
    use std::collections::BTreeSet;

    #[test]
    fn the_switch_is_off_unless_it_says_on() {
        assert_eq!(ReviewProposalRule::default(), ReviewProposalRule::Off);
        assert_eq!(ReviewProposalRule::parse(None), ReviewProposalRule::Off);
        for on in ["on", "ON", "On", " on\n", "\ton "] {
            assert_eq!(
                ReviewProposalRule::parse(Some(on)),
                ReviewProposalRule::On,
                "{on:?}"
            );
        }
        for off in ["off", "OFF", " off ", "", " ", "\n"] {
            assert_eq!(
                ReviewProposalRule::parse(Some(off)),
                ReviewProposalRule::Off,
                "{off:?}"
            );
        }
        // Anything else, the latency levers' "on" words included, is off.
        for other in [
            "1",
            "true",
            "yes",
            "enabled",
            "v2",
            "o n",
            "on2",
            "onn",
            "0",
            "no",
            "disabled",
            "proposal-rule-1",
        ] {
            assert!(
                ReviewProposalRule::recognise(other).is_none(),
                "{other:?} is not a setting"
            );
            assert_eq!(
                ReviewProposalRule::parse(Some(other)),
                ReviewProposalRule::Off,
                "{other:?}"
            );
        }
        assert_eq!(
            ReviewProposalRule::ALL.map(ReviewProposalRule::name),
            ["off", "on"]
        );
        for rule in ReviewProposalRule::ALL {
            assert_eq!(ReviewProposalRule::parse(Some(rule.name())), rule);
        }
    }

    /// The process value is read once. Without the variable set it is off;
    /// with it set it is what it says.
    #[test]
    fn the_process_setting_is_read_from_the_variable() {
        let expected =
            ReviewProposalRule::parse(std::env::var(REVIEW_PROPOSAL_RULE_ENV).ok().as_deref());
        assert_eq!(ReviewProposalRule::from_env(), expected);
        assert_eq!(
            ReviewProposalRule::from_env(),
            ReviewProposalRule::from_env()
        );
    }

    /// Off adds nothing and records nothing; on adds the rule and records its id.
    #[test]
    fn only_on_adds_the_rule_and_names_it() {
        assert_eq!(ReviewProposalRule::Off.instruction(), None);
        assert_eq!(ReviewProposalRule::Off.receipt_id(), None);
        assert_eq!(ReviewProposalRule::On.instruction(), Some(PROPOSAL_RULE));
        assert_eq!(ReviewProposalRule::On.receipt_id(), Some(PROPOSAL_RULE_ID));
    }

    fn sha256(text: &str) -> String {
        use sha2::Digest;
        use std::fmt::Write as _;
        sha2::Sha256::digest(text.as_bytes())
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").expect("writing to a String");
                hex
            })
    }

    /// The text is pinned to its id. Editing the rule must also give it a new
    /// id, so a receipt never names a rule other than the one that checked it.
    #[test]
    fn a_changed_rule_needs_a_new_id() {
        assert_eq!(
            (PROPOSAL_RULE_ID, sha256(PROPOSAL_RULE), PROPOSAL_RULE.len()),
            (
                "proposal-rule-2",
                "7761b135cb74593f5ccddd5a7199719cb0a51781d24beae702bb0c39a7cf218f".to_owned(),
                3045
            )
        );
        // One line of plain ASCII, so it embeds in the instruction unchanged.
        assert!(PROPOSAL_RULE.is_ascii());
        assert!(!PROPOSAL_RULE.contains(['\n', '\r', '"']));
    }

    /// Rule-2 keeps what rule-1 decided and closes what the review of rule-1
    /// found open; each sentence below is one of those decisions.
    #[test]
    fn rule_two_states_each_decision_of_its_review() {
        for sentence in [
            // A label never excuses a factual assertion inside the proposal
            // (the accepted source-role proposals differ from their rejected
            // twins only by the label, so the label may make a recommendation
            // acceptable).
            "A label never makes acceptable a factual assertion inside the proposal that would be unsupported without it.",
            // Proposals are compared with the other segments too.
            "conflicts with no other segment, other proposals included, for example by forming a circular or impossible sequence with them;",
            // Source text alone opens the role exception only with no request.
            "Proposed roles are requested only when the user's request asks for proposed owners or roles, or when the task adopts a supplied brief that asks for them; with no request supplied, such a brief suffices.",
            "Asking for each task's responsible role does not by itself request proposed roles.",
            // An order of steps is not a decision time limit.
            "Proposing only that one step come before another sets no time limit for a decision and is judged by the three criteria.",
            // D4 for ranges: a range or a set-aside period is a duration.
            "A date range or a period set aside states a duration, which needs a duration the sources give even where a single later date may be chosen.",
            "A heading, a lead-in line or a document-wide draft label makes nothing a proposal;",
            "Adding a proposal label never makes acceptable anything this paragraph rejects, and a suggested edit must not add a label to make a rejected claim pass.",
        ] {
            assert_eq!(PROPOSAL_RULE.matches(sentence).count(), 1, "{sentence}");
        }
        // Rule-1's contradicted and open sentences are gone.
        for gone in [
            "A label never makes acceptable what would be unsupported without it.",
            "unless the request or a supplied brief asks for proposed owners or roles",
            "Adding a proposal label never corrects an unsupported claim",
        ] {
            assert!(!PROPOSAL_RULE.contains(gone), "{gone}");
        }
    }

    /// Every text the proposal-rule controls send: each source, request and
    /// candidate of the matrix controls and each part of the recheck and
    /// relabel controls.
    fn control_texts() -> Vec<(String, String)> {
        let mut texts = Vec::new();
        for control in controls::matrix_controls() {
            for (name, content) in control.sources {
                texts.push((format!("{} source {name}", control.id), (*content).to_owned()));
            }
            if let Some(request) = control.request {
                texts.push((format!("{} request", control.id), request.to_owned()));
            }
            texts.push((format!("{} candidate", control.id), control.candidate));
        }
        for (id, source, faulty, dependent, replacement, _, _) in controls::RECHECK_CONTROLS {
            for (part, text) in [
                ("source", source),
                ("faulty", faulty),
                ("dependent", dependent),
                ("replacement", replacement),
            ] {
                texts.push((format!("{id} {part}"), text.to_owned()));
            }
        }
        for (id, source, faulty, dependent, replacement, _) in controls::RELABEL_CONTROLS {
            for (part, text) in [
                ("source", source),
                ("faulty", faulty),
                ("dependent", dependent),
                ("replacement", replacement),
            ] {
                texts.push((format!("{id} {part}"), text.to_owned()));
            }
        }
        texts
    }

    /// The rule and its controls must work without the fixtures' facts or the
    /// oracle's answer key: no name, company, identifier, date, month,
    /// weekday, year, number, quotation or role from the four journeys'
    /// prompts, source packs and director notes or from the answer key, no
    /// oracle stem, no fixture topic, and no quoted fixture phrase.
    #[test]
    fn the_rule_and_its_controls_carry_no_fixture_or_oracle_content() {
        let terms = leakage::fixture_terms();
        for kind in ["name", "date", "month", "number", "role"] {
            assert!(
                terms.get(kind).is_some_and(|terms| !terms.is_empty()),
                "no {kind} was collected from the fixtures"
            );
        }
        let fixtures = leakage::normalise(&leakage::fixture_text());
        // The guard is live: a planted fixture name or date is caught.
        for planted in [
            format!("{PROPOSAL_RULE} Nora"),
            format!("{PROPOSAL_RULE} 30. september"),
            format!(
                "{} Ask Fjordform.",
                controls::matrix_controls()[0].candidate
            ),
            format!("{} 24 March", controls::HALL),
        ] {
            assert!(!leakage::leaks(&planted, &terms).is_empty(), "{planted}");
        }

        assert_eq!(leakage::leaks(PROPOSAL_RULE, &terms), Vec::<String>::new());
        assert_eq!(leakage::topic_stems(PROPOSAL_RULE), BTreeSet::new());
        let quoted = leakage::quoted_phrases(PROPOSAL_RULE);
        assert_eq!(
            quoted
                .iter()
                .filter(|phrase| fixtures.contains(phrase.as_str()))
                .collect::<Vec<_>>(),
            Vec::<&String>::new()
        );

        let texts = control_texts();
        // 71 candidates, 75 sources (the four brief controls send two), six
        // requests, and four parts of each of the 2 + 4 recheck controls.
        assert_eq!(texts.len(), 71 + 75 + 6 + 4 * (2 + 4), "every control is checked");
        for (name, text) in texts {
            assert_eq!(
                leakage::leaks(&text, &terms),
                Vec::<String>::new(),
                "{name}"
            );
            assert_eq!(leakage::topic_stems(&text), BTreeSet::new(), "{name}");
            let copied: Vec<_> = leakage::quoted_phrases(&text)
                .into_iter()
                .filter(|phrase| fixtures.contains(phrase.as_str()))
                .collect();
            assert_eq!(copied, Vec::<String>::new(), "{name}");
        }
    }

    /// The controls are the paired sets the two design reviews proposed: 71
    /// matrix controls (25 accept, 46 reject; 25 `pr1-` and 46 `pr2-`), 2
    /// recheck and 4 relabel controls, all with labels still marked proposed,
    /// and unique ids that cannot collide with the existing cases.
    #[test]
    fn the_controls_are_the_proposed_paired_set() {
        assert_eq!(controls::LABEL_STATUS, "proposed");
        let matrix = controls::matrix_controls();
        assert_eq!(matrix.len(), 71);
        assert_eq!(matrix.iter().filter(|control| control.expected).count(), 25);
        let ids: BTreeSet<_> = matrix.iter().map(|control| control.id).collect();
        assert_eq!(ids.len(), matrix.len());
        assert_eq!(ids.iter().filter(|id| id.starts_with("pr1-")).count(), 25);
        assert_eq!(ids.iter().filter(|id| id.starts_with("pr2-")).count(), 46);
        // Only the brief and request controls are played as documents.
        let requested: Vec<_> = matrix.iter().filter(|control| control.request.is_some()).map(|control| control.id).collect();
        assert_eq!(requested.len(), 6);
        assert!(requested.iter().all(|id| id.starts_with("pr2-brief-") || id.starts_with("pr2-request-")), "{requested:?}");
        // A table-row rejection must reject the row itself, and the batched
        // rejection both the unsupported fact and the proposal relying on it.
        let required: Vec<(&str, &[u64])> = matrix
            .iter()
            .filter(|control| !control.required_rejections.is_empty())
            .map(|control| (control.id, control.required_rejections))
            .collect();
        assert_eq!(
            required,
            [
                ("pr1-row-invented-role", &[1][..]),
                ("pr1-row-unlabelled-assignment", &[1]),
                ("pr1-row-presupposed-completion", &[1]),
                ("pr1-row-value-mislabelled-calculated", &[1]),
                ("pr1-rad-uten-forslagsmerke", &[1]),
                ("pr1-batched-reliance-unsupported", &[1, 22]),
                ("pr2-sched-signs-range", &[2]),
                ("pr2-sched-signs-spare-days", &[2]),
                ("pr2-sched-signs-date-labelled-calculated", &[2]),
                ("pr2-sched-cleaning-earlier-date", &[1]),
                ("pr2-sched-header-range", &[2]),
                ("pr2-sched-header-spare-days", &[2]),
                ("pr2-attributed-wrong-proposer", &[2]),
                ("pr2-header-owner-person", &[1]),
                ("pr2-header-owner-approver", &[1]),
                ("pr2-header-owner-duration", &[1]),
                ("pr2-header-owner-presupposed", &[1]),
                ("pr2-header-follow-up-unlabelled", &[1]),
                ("pr2-heading-unlabelled", &[1]),
                ("pr2-standalone-duration", &[1]),
                ("pr2-brief-adopted-person", &[1]),
                ("pr2-brief-adopted-approver", &[1]),
                ("pr2-brief-not-adopted", &[1]),
                ("pr2-request-responsible-role", &[1]),
            ]
        );
        for control in &matrix {
            let parts = crate::source_validation::segments_for_test(&control.candidate, crate::latency_levers::LatencyLevers::ALL_ON);
            for &index in control.required_rejections {
                // A required index is a real segment, and in a table a row.
                let part = usize::try_from(index).ok().and_then(|index| parts.get(index)).unwrap_or_else(|| panic!("{}: no segment {index}", control.id));
                if part.starts_with('|') {
                    assert_eq!(part.lines().count(), 1, "{}: segment {index} is a table header", control.id);
                }
            }
            if control.id.contains("-batched-") {
                // Enough segments for four batches, the proposal last.
                assert_eq!(parts.len(), 23, "{}", control.id);
            }
        }
        assert_eq!(
            controls::RECHECK_CONTROLS.map(|(id, .., expected, _)| (id, expected)),
            [
                ("proposal-reference-preserved", true),
                ("proposal-reference-changed", false)
            ]
        );
        assert_eq!(
            controls::RELABEL_CONTROLS.map(|(id, .., expected)| (id, expected)),
            [
                ("proposal-relabel-signoff-corrected", true),
                ("proposal-relabel-signoff", false),
                ("proposal-relabel-duration-corrected", true),
                ("proposal-relabel-duration", false),
            ]
        );
        let recheck_ids: BTreeSet<_> = controls::RECHECK_CONTROLS.iter().map(|control| control.0)
            .chain(controls::RELABEL_CONTROLS.iter().map(|control| control.0)).collect();
        assert_eq!(recheck_ids.len(), 6);
        assert!(recheck_ids.is_disjoint(&ids));
    }

    /// Each reject control names an accept twin with the same attachments,
    /// and differs from it in one line: one clause of the candidate with the
    /// same request, or the request with the same candidate. It also names
    /// words its rejection reason should contain. An accept control names
    /// neither and requires no rejection.
    #[test]
    fn every_reject_control_differs_from_its_accept_twin_in_one_line() {
        let matrix = controls::matrix_controls();
        let by_id: std::collections::BTreeMap<_, _> = matrix.iter().map(|control| (control.id, control)).collect();
        let mut twins = BTreeSet::new();
        for control in &matrix {
            if control.expected {
                assert!(control.twin.is_none() && control.reason_terms.is_empty() && control.required_rejections.is_empty(), "{}", control.id);
                continue;
            }
            let twin = by_id[control.twin.unwrap_or_else(|| panic!("{} names no twin", control.id))];
            twins.insert(twin.id);
            assert!(twin.expected, "{}: its twin {} is a reject control", control.id, twin.id);
            assert_eq!(control.sources, twin.sources, "{}", control.id);
            assert!(!control.reason_terms.is_empty(), "{}", control.id);
            assert!(control.reason_terms.iter().all(|term| !term.trim().is_empty() && *term == term.to_lowercase()), "{}", control.id);
            if control.candidate == twin.candidate {
                assert!(control.request.is_some() && twin.request.is_some() && control.request != twin.request, "{}: identical to its twin", control.id);
            } else {
                assert_eq!(control.request, twin.request, "{}", control.id);
                let (lines, twin_lines): (Vec<_>, Vec<_>) = (control.candidate.lines().collect(), twin.candidate.lines().collect());
                assert_eq!(lines.len(), twin_lines.len(), "{}", control.id);
                let changed = lines.iter().zip(&twin_lines).filter(|(line, twin_line)| line != twin_line).count();
                assert_eq!(changed, 1, "{}: differs from {} in {changed} lines", control.id, twin.id);
            }
        }
        // Every pr2 accept control is some reject control's twin, and so is
        // each pr1 accept control but the row clarification.
        let untwinned: Vec<_> = matrix.iter().filter(|control| control.expected && !twins.contains(control.id)).map(|control| control.id).collect();
        assert_eq!(untwinned, ["pr1-row-clarify-valid"]);
    }

    /// A relabel control's scripted repair only adds a proposal label to the
    /// rejected claim; its corrected twin replaces the claim with a source
    /// fact. Each pair shares its source, rejected claim and dependent text.
    #[test]
    fn relabel_controls_only_add_a_label() {
        for pair in controls::RELABEL_CONTROLS.chunks(2) {
            let [(corrected, source, faulty, dependent, fix, true), (relabelled, twin_source, twin_faulty, twin_dependent, label, false)] = pair else {
                panic!("relabel controls come as (corrected, relabelled) pairs");
            };
            assert_eq!((source, faulty, dependent), (twin_source, twin_faulty, twin_dependent), "{corrected} {relabelled}");
            let mut lowered = faulty.chars();
            let first = lowered.next().expect("a claim").to_lowercase().collect::<String>();
            assert_eq!(*label, format!("Proposal: {first}{}", lowered.as_str()), "{relabelled}");
            assert!(!fix.starts_with("Proposal:") && source.lines().any(|line| line == *fix), "{corrected}: the fix is a source line");
            assert_ne!(faulty, dependent);
        }
    }

    fn schedule_context(sources: controls::Sources) -> crate::source_validation::SourceContext {
        crate::source_validation::SourceContext {
            sources: sources
                .iter()
                .enumerate()
                .map(|(id, (name, content))| crate::source_validation::Source {
                    id,
                    name: (*name).to_owned(),
                    content: (*content).to_owned(),
                })
                .collect(),
        }
    }

    /// The scheduled controls' source gets a computed schedule, and its dates
    /// are the ones the controls call calculated; no other control source gets
    /// one, so only the scheduled controls carry the computedSchedule note.
    #[test]
    fn the_scheduled_controls_get_the_computed_schedule_they_quote() {
        let schedule = crate::schedule_evidence::computed_schedule(&schedule_context(controls::HALL_SOURCES)).expect("an explicit workday chain");
        let task = |id: &str| schedule["tasks"].as_array().unwrap().iter().find(|task| task["id"] == id).unwrap_or_else(|| panic!("no task {id}")).clone();
        assert_eq!(task("palette")["possibleApproval"], "2031-03-03");
        assert_eq!(task("priming")["dates"]["workDates"], serde_json::json!(["2031-03-04", "2031-03-05"]));
        assert_eq!(task("painting")["dates"]["workDates"], serde_json::json!(["2031-03-06", "2031-03-07"]));
        // "calculated lower bound if the palette is approved on 3 March: 10 March".
        assert_eq!(task("cleaning")["dates"]["earliestStart"], "2031-03-10");
        assert_eq!(schedule["unestimatedTasks"], serde_json::json!([]));
        for sources in [controls::MOVE_SOURCES, controls::VERKSTED_SOURCES, controls::BRIEF_SOURCES] {
            assert!(crate::schedule_evidence::computed_schedule(&schedule_context(sources)).is_none());
        }
        let scheduled: Vec<_> = controls::matrix_controls().into_iter().filter(|control| control.sources == controls::HALL_SOURCES).map(|control| control.id).collect();
        assert_eq!(scheduled.len(), 12);
        assert!(scheduled.iter().all(|id| id.starts_with("pr2-sched-") || id.starts_with("pr2-attributed-")), "{scheduled:?}");
    }

    /// Every control reaches the reviewer in both rule states, with the
    /// levers and qualifier scope of the code defaults and of the shipped
    /// settings: no local guard decides it before the model. Its one review
    /// request carries the rule only when on, the computedSchedule note exactly
    /// when its source has a schedule, and the request (with the brief's
    /// adoption rule) exactly when the control is played as a document.
    #[tokio::test(start_paused = true)]
    async fn every_control_reaches_the_reviewer_with_what_it_tests() {
        use crate::latency_levers::LatencyLevers;
        use crate::source_facts::QualifierScope;
        use crate::source_validation::CheckSettings;
        use mp_contracts::model_plane::v1::{ChatMessage, InferRequest, InferResponse};
        const SCHEDULE_NOTE: &str = "A later proposed date is allowed when identified as a choice";
        const ADOPTION: &str = "A source brief or language profile supplies requirements only when the user's task adopts it.";
        for rule in ReviewProposalRule::ALL {
            for (levers, qualifier_scope) in [(LatencyLevers::ALL_ON, QualifierScope::Line), (LatencyLevers::ALL_OFF, QualifierScope::Clause)] {
                let settings = CheckSettings { levers, qualifier_scope, proposal_rule: rule };
                for control in controls::matrix_controls() {
                    let context = schedule_context(control.sources);
                    let mut request = InferRequest { request_id: "req".into(), model: "gpt-5.6-terra".into(), provider_hint: "openai-codex-subscription".into(),
                        subscription_connection_id: "selected".into(), org_id: "test-org".into(), ..Default::default() };
                    let sent = std::cell::RefCell::new(Vec::<InferRequest>::new());
                    let infer = |request: InferRequest| {
                        sent.borrow_mut().push(request);
                        std::future::ready(Err::<InferResponse, _>(tonic::Status::unavailable("scripted stop after the first review request")))
                    };
                    let stopped = if let Some(prompt) = control.request {
                        request.messages = vec![ChatMessage { role: "user".into(), content: prompt.into(), ..Default::default() }];
                        crate::source_validation::check_artifact(settings, Some(&context), &request, prompt, None, None, &control.candidate, infer).await.map(|_| ())
                    } else {
                        let mut infer = infer;
                        crate::source_validation::review_with(settings, &context, &request, &control.candidate, &mut infer).await.map(|_| ())
                    };
                    assert_eq!(stopped.map_err(|status| status.code()), Err(tonic::Code::Unavailable), "{} ({settings:?}): decided before the model", control.id);
                    // The first review only: one call, or one per batch of a
                    // split review, sent together.
                    let sent = sent.into_inner();
                    let batched = control.id.contains("-batched-");
                    assert_eq!(sent.len(), if batched { 4 } else { 1 }, "{}", control.id);
                    for request in &sent {
                        assert!(request.request_id.contains("-source-review-"), "{}: {}", control.id, request.request_id);
                        assert_eq!(request.request_id.contains("-batch-"), batched, "{}: {}", control.id, request.request_id);
                        let system = &request.messages[0].content;
                        let data: serde_json::Value = serde_json::from_str(&request.messages[1].content).unwrap();
                        assert_eq!(system.matches(PROPOSAL_RULE).count(), usize::from(rule == ReviewProposalRule::On), "{}", control.id);
                        let scheduled = control.sources == controls::HALL_SOURCES;
                        assert_eq!(data["computedSchedule"].is_object(), scheduled, "{}", control.id);
                        assert_eq!(system.contains(SCHEDULE_NOTE), scheduled, "{}", control.id);
                        assert_eq!(data["sources"].as_array().map(Vec::len), Some(control.sources.len()), "{}", control.id);
                        assert_eq!(data["segments"].as_array().map(Vec::len), Some(crate::source_validation::segments_for_test(&control.candidate, levers).len()), "{}", control.id);
                        if let Some(prompt) = control.request {
                            assert_eq!(data["activeRequest"], prompt, "{}", control.id);
                            assert_eq!(data["userRequests"], serde_json::json!([prompt]), "{}", control.id);
                            assert!(system.contains(ADOPTION), "{}", control.id);
                        } else {
                            assert!(data["activeRequest"].is_null(), "{}", control.id);
                            assert_eq!(data["userRequests"], serde_json::json!([]), "{}", control.id);
                            assert!(!system.contains(ADOPTION), "{}", control.id);
                        }
                    }
                }
            }
        }
    }

    fn row(id: &str, accepted: Option<bool>, correct: bool, twin: Option<&str>, names_change: bool, label_status: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "accepted": accepted, "correct": correct, "twin": twin, "reasonNamesChange": names_change, "labelStatus": label_status})
    }

    /// A reject control is evidence only when it was rejected, its twin was
    /// accepted in the same run and its reason names the changed clause; a run
    /// without the twin cannot tell.
    #[test]
    fn twin_conditional_evidence_needs_the_twin_accepted_and_the_clause_named() {
        use serde_json::Value::{Bool, Null};
        let mut rows = vec![
            row("valid", Some(true), true, None, false, "proposed"),
            row("rejected-valid", Some(false), false, None, false, "proposed"),
            row("named", Some(false), true, Some("valid"), true, "proposed"),
            row("other-reason", Some(false), true, Some("valid"), false, "proposed"),
            row("twin-rejected", Some(false), true, Some("rejected-valid"), true, "proposed"),
            row("accepted", Some(true), false, Some("valid"), false, "proposed"),
            row("twin-absent", Some(false), true, Some("filtered-out"), true, "proposed"),
            row("protocol-error", None, false, Some("valid"), false, "proposed"),
        ];
        controls::annotate_twins(&mut rows);
        let got: Vec<_> = rows.iter().map(|row| (row["id"].as_str().unwrap(), row["twinAccepted"].clone(), row["evidence"].clone())).collect();
        assert_eq!(got, [
            ("valid", Null, Null),
            ("rejected-valid", Null, Null),
            ("named", Bool(true), Bool(true)),
            ("other-reason", Bool(true), Bool(false)),
            ("twin-rejected", Bool(false), Bool(false)),
            ("accepted", Bool(true), Bool(false)),
            ("twin-absent", Null, Null),
            ("protocol-error", Bool(true), Bool(false)),
        ]);
        // Rows without a twin get no evidence fields.
        assert!(rows[0].get("evidence").is_none() && rows[1].get("evidence").is_none());
    }

    /// Only rows with confirmed labels decide a run: a proposed control's miss
    /// is reported, never counted, and a confirmed one fails on a wrong verdict
    /// or on false twin-conditional evidence.
    #[test]
    fn only_confirmed_labels_fail_a_run() {
        let mut rows = vec![
            row("original-ok", Some(true), true, None, false, "confirmed"),
            row("original-miss", Some(false), false, None, false, "confirmed"),
            row("proposed-miss", Some(true), false, None, false, "proposed"),
            row("proposed-twin", Some(true), true, None, false, "proposed"),
            row("proposed-unnamed", Some(false), true, Some("proposed-twin"), false, "proposed"),
            row("confirmed-twin", Some(true), true, None, false, "confirmed"),
            row("confirmed-unnamed", Some(false), true, Some("confirmed-twin"), false, "confirmed"),
            row("confirmed-named", Some(false), true, Some("confirmed-twin"), true, "confirmed"),
            row("confirmed-no-twin-run", Some(false), true, Some("absent"), true, "confirmed"),
        ];
        controls::annotate_twins(&mut rows);
        assert_eq!(controls::gating_failures(&rows), ["original-miss", "confirmed-unnamed"]);
    }

    #[test]
    fn rejection_reasons_are_read_from_both_wire_formats() {
        let responses = [
            serde_json::json!({"content": r#"{"checks":[{"i":0,"v":"s","e":[],"r":""},{"i":1,"v":"u","e":[],"r":"Names Mira Holt, whom the notes never name."}]}"#}),
            serde_json::json!({"content": r#"{"checks":[{"index":1,"status":"unsupported","evidence":[],"reason":"Sets a decision DEADLINE."}]}"#}),
            serde_json::json!({"content": "not json"}),
        ];
        let reasons = controls::rejection_reasons(&responses);
        assert_eq!(reasons, ["Names Mira Holt, whom the notes never name.", "Sets a decision DEADLINE."]);
        assert!(controls::reason_names_change(&reasons, &["holt"]));
        assert!(controls::reason_names_change(&reasons, &["deadline"]));
        assert!(!controls::reason_names_change(&reasons, &["duration", "march"]));
        assert!(!controls::reason_names_change(&[], &["holt"]));
    }
}
