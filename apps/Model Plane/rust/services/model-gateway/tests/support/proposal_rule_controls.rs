//! Paired controls for the source-review proposal rule
//! (`model_gateway::review_proposal_rule`, now `proposal-rule-2`).
//!
//! Every label here is PROPOSED, none confirmed. The `pr1-` controls come from
//! the rule's design review (2026-09-26); the `pr2-` controls and the relabel
//! recheck controls come from the review of that design, which found clauses,
//! label forms and paths the `pr1-` set never reached: a source with a
//! computed schedule (so the note that allows a chosen later date is present),
//! column-header labels, a brief or a request that asks for proposed roles
//! (played through `check_artifact`, so the reviewer gets `userRequests`),
//! proposals that conflict with each other, and a repair that only adds a
//! label. The owner confirms or corrects each label before the first gate run,
//! and no label changes in response to a result afterwards: a label corrected
//! after results gets a new id with the next free prefix. Until then
//! [`LABEL_STATUS`] stays `proposed`, the harnesses record it on every row, and
//! these controls do not decide a run's exit status ([`gating_failures`]).
//!
//! All content is invented: no fact, name, date, role or answer-key concept of
//! the maintained product-recording fixtures. Each reject control names its
//! accept twin and differs from it in one line only: one clause of the
//! candidate, or the request. A rejection counts as evidence that the rule's
//! clause was applied only when the twin was accepted in the same run and a
//! rejection reason names the changed clause ([`annotate_twins`]); a rejection
//! for another reason is not that evidence. The guard tests in
//! `src/review_proposal_rule.rs` check the pairing, check every text here
//! against the fixtures and the acceptance oracle, and check that every control
//! reaches the reviewer rather than a local guard.
//!
//! Shared by `tests/source_review_live.rs`, `tests/source_recheck_live.rs` and
//! that guard, so the guard checks exactly what the live harnesses send.
#![allow(dead_code)]

use serde_json::Value;

/// Whether the labels below are confirmed by the owner.
pub const LABEL_STATUS: &str = "proposed";

/// The harnesses' name for the rows of these controls.
pub const CASE_SET: &str = "proposal-rule";

/// The attachment name the controls' notes are sent under.
pub const SOURCE_NAME: &str = "proposal-notes.txt";

/// The attachment name of the brief ([`BRIEF`]).
pub const BRIEF_NAME: &str = "move-brief.txt";

/// Fictional sources for the proposal-rule controls.
pub const MOVE: &str = "Fictional notes for moving a records archive.\nThe records team packs the archive boxes.\nPacking takes two working days and may start only after the shelf survey is signed off.\nThe sign-off date for the shelf survey is not stated.\nThe site coordinator books the goods lift.\nNo one is named to label the boxes.\nThe move may start only after every box is labelled and the goods lift is booked.\nThe goods lift may be closed for maintenance, which could delay the move; no maintenance date has been announced.\nNo additional staff can be hired for the move.\nThe records manager decides the move date; no decision date is set.";
pub const VERKSTED: &str = "Fiktive notater for flytting av et verksted.\nVerkstedteamet demonterer maskinene.\nDemonteringen tar tre arbeidsdager og kan først starte når strømmen er koblet fra.\nDatoen for frakobling av strømmen er ikke oppgitt, og det kan forsinke demonteringen.\nDriftskoordinatoren bestiller kranbilen.\nIngen er utpekt til å merke delene.\nFlyttingen kan først starte når alle deler er merket og kranbilen er bestilt.\nDet kan ikke leies inn ekstra personell.\nVerkstedeieren bestemmer flyttedatoen; ingen beslutningsdato er satt.";
/// One year and an explicit workday chain, so the gateway computes a schedule
/// and the review gets the computedSchedule note ("A later proposed date is
/// allowed when identified as a choice"). If the palette is approved on
/// Monday 3 March, priming is 4-5 March, painting 6-7 March and cleaning
/// 10 March. The signs have no owner and no duration.
pub const HALL: &str = "Fictional notes for repainting a community hall in 2031.\nWorkdays are Monday-Friday.\nApproval at end of day permits dependent work the next workday.\nThe palette may be approved on 3 March.\nPainters: Priming needs two full workdays after approved palette.\nPainters: Painting needs two full workdays after completed priming.\nCaretakers can start cleaning after priming and painting are complete. Cleaning needs one full workday.\nThe caretaker proposes reopening the hall on 20 March; the reopening date is not decided.\nNew signs are needed for the hall; no one is named to make them, and the time needed to make them is not stated.";
/// A brief, sent as a second attachment beside [`MOVE`], that asks for
/// proposed owner roles.
pub const BRIEF: &str = "Brief for the move plan: for every task that has no owner in the notes, propose a role as its owner and mark it as a proposal.";

/// The attachments of a control, as (name, content).
pub type Sources = &'static [(&'static str, &'static str)];
pub const MOVE_SOURCES: Sources = &[(SOURCE_NAME, MOVE)];
pub const VERKSTED_SOURCES: Sources = &[(SOURCE_NAME, VERKSTED)];
pub const HALL_SOURCES: Sources = &[(SOURCE_NAME, HALL)];
pub const BRIEF_SOURCES: Sources = &[(SOURCE_NAME, MOVE), (BRIEF_NAME, BRIEF)];

const MOVE_RISK: &str = "- Risk: the goods lift may be closed for maintenance, which could delay the move; no maintenance date has been announced. Proposal: ask the site coordinator to check whether maintenance is planned before booking the lift.";
const MOVE_GAP: &str = "- Gap: no one is named to label the boxes, and the move may start only after every box is labelled. Proposal: clarify who labels the boxes before packing starts.";
const MOVE_DECISION: &str = "- Decision: the records manager decides the move date; no decision date is set. Proposal: record the decision when it is taken.";
const MOVE_TABLE: &str =
    "| Task | Responsible role | Depends on | Done when |\n|---|---|---|---|\n";
const MOVE_OWNER_TABLE: &str = "| Task | Responsible role | Proposed owner |\n|---|---|---|\n";
/// A header that labels nothing: the boundary for [`MOVE_OWNER_TABLE`].
const MOVE_FOLLOW_UP_TABLE: &str = "| Task | Responsible role | Follow-up |\n|---|---|---|\n";
const MOVE_LAST: &str = "- Risk: the goods lift may be closed for maintenance, which could delay the move; no maintenance date has been announced. Proposal: ask the role named above as booking the lift to check whether maintenance is planned before the booking.";
const VERKSTED_RISK: &str = "- Risiko: datoen for frakobling av strømmen er ikke oppgitt, og det kan forsinke demonteringen. Forslag: avklar datoen for frakoblingen før kranbilen bestilles.";
const VERKSTED_GAP: &str = "- Avklaring: ingen er utpekt til å merke delene, og flyttingen kan først starte når alle deler er merket. Forslag: avklar hvem som merker delene før demonteringen starter.";
const VERKSTED_TABLE: &str =
    "| Oppgave | Ansvarlig rolle | Avhengigheter | Ferdigkriterium |\n|---|---|---|---|\n";
const HALL_TABLE: &str = "| Task | Responsible role | Depends on | Date |\n|---|---|---|---|\n";
/// The same table with its date column labelled as proposals by its header.
const HALL_PROPOSED_TABLE: &str =
    "| Task | Responsible role | Depends on | Proposed date |\n|---|---|---|---|\n";
/// The source's calculated lower bound, with its condition.
const HALL_CLEANING: &str = "| Cleaning | Caretakers | priming and painting | calculated lower bound if the palette is approved on 3 March: 10 March |";
/// The labelled request of the brief controls, and the request that does not
/// adopt the brief.
const ADOPTS_BRIEF: &str =
    "Add the box-labelling task to the move plan table, following the supplied brief.";
const IGNORES_BRIEF: &str =
    "Add the box-labelling task to the move plan table, using only the move notes.";
const ASKS_PROPOSED_OWNER: &str = "Add the box-labelling task to the move plan table and propose an owner role if the notes name none.";
const ASKS_RESPONSIBLE_ROLE: &str =
    "Add the box-labelling task to the move plan table with its responsible role.";
/// A role the sources never name, proposed as the labelling owner.
const VOLUNTEER_ROW: &str =
    "| Label the boxes | not stated; proposal: a volunteer group from the building | not stated | not stated |";

/// One labelled matrix control, as `tests/source_review_live.rs` plays it.
pub struct MatrixControl {
    pub id: &'static str,
    pub sources: Sources,
    /// The user's request. `Some` plays the control as a document through
    /// `check_artifact`, which sends the request as `activeRequest` and
    /// `userRequests`; `None` plays it through `review()`, which sends no
    /// request, as every `pr1-` control does.
    pub request: Option<&'static str>,
    pub candidate: String,
    /// The proposed label: whether the review should accept the candidate.
    pub expected: bool,
    /// Segment indexes a correct rejection must reject, beyond the verdict.
    pub required_rejections: &'static [u64],
    /// A reject control's accept twin, which it differs from in one line.
    pub twin: Option<&'static str>,
    /// Words of a reject control's changed clause, any case. A rejection
    /// reason that names the clause contains one of them; part of the label.
    pub reason_terms: &'static [&'static str],
}

fn accept(id: &'static str, sources: Sources, candidate: String) -> MatrixControl {
    MatrixControl {
        id,
        sources,
        request: None,
        candidate,
        expected: true,
        required_rejections: &[],
        twin: None,
        reason_terms: &[],
    }
}

fn reject(
    id: &'static str,
    sources: Sources,
    candidate: String,
    twin: &'static str,
    required_rejections: &'static [u64],
    reason_terms: &'static [&'static str],
) -> MatrixControl {
    MatrixControl {
        id,
        sources,
        request: None,
        candidate,
        expected: false,
        required_rejections,
        twin: Some(twin),
        reason_terms,
    }
}

/// `control` played through `check_artifact` with `request`.
fn requested(request: &'static str, control: MatrixControl) -> MatrixControl {
    MatrixControl {
        request: Some(request),
        ..control
    }
}

/// The 71 matrix controls: 25 accept, 46 reject.
#[allow(clippy::too_many_lines)] // One entry per control; splitting it would scatter the pairs.
pub fn matrix_controls() -> Vec<MatrixControl> {
    let bullets = |risk: &str, gap: &str, decision: &str| format!("{risk}\n{gap}\n{decision}");
    let gap = |proposal: &str| {
        MOVE_GAP.replacen(
            "clarify who labels the boxes before packing starts",
            proposal,
            1,
        )
    };
    let punkter = |proposal: &str| {
        format!(
            "{VERKSTED_RISK}\n{}",
            VERKSTED_GAP.replacen(
                "avklar hvem som merker delene før demonteringen starter",
                proposal,
                1
            )
        )
    };
    let row = |cells: &str| format!("{MOVE_TABLE}{cells}");
    let rad = |cells: &str| format!("{VERKSTED_TABLE}{cells}");
    let owner = |cell: &str| format!("{MOVE_OWNER_TABLE}| Label the boxes | not stated | {cell} |");
    // Scheduled tables: the calculated cleaning row (index 1) and one more row
    // (index 2).
    let signs = |cell: &str| format!("{HALL_TABLE}{HALL_CLEANING}\n| Signs | not stated | not stated | {cell} |");
    let cleaning = |cell: &str| {
        format!("{HALL_TABLE}| Cleaning | Caretakers | priming and painting | {cell} |\n| Signs | not stated | not stated | not stated |")
    };
    let proposed_signs = |cell: &str| {
        format!("{HALL_PROPOSED_TABLE}| Cleaning | Caretakers | priming and painting | 11 March, later than the calculated lower bound of 10 March if the palette is approved on 3 March |\n| Signs | not stated | not stated | {cell} |")
    };
    let reopening = |cell: &str| format!("{HALL_TABLE}{HALL_CLEANING}\n| Reopening | not stated | not stated | {cell} |");
    let later = "calculated lower bound if the palette is approved on 3 March: 10 March; proposal (chosen date): ";
    let mut controls = vec![
        // pr1: the rule's design review.
        accept("pr1-bullets-valid", MOVE_SOURCES, bullets(MOVE_RISK, MOVE_GAP, MOVE_DECISION)),
        accept("pr1-bullets-keep-condition-valid", MOVE_SOURCES, bullets(MOVE_RISK, &gap("keep the move conditional on every box being labelled and the lift being booked"), MOVE_DECISION)),
        reject("pr1-bullets-invented-date", MOVE_SOURCES, bullets(MOVE_RISK, &gap("have the boxes labelled by 6 March"), MOVE_DECISION), "pr1-bullets-valid", &[], &["march", "date", "deadline"]),
        reject("pr1-bullets-invented-duration", MOVE_SOURCES, bullets(MOVE_RISK, &gap("set aside one working day for labelling the boxes"), MOVE_DECISION), "pr1-bullets-valid", &[], &["working day", "one day", "duration"]),
        reject("pr1-bullets-invented-person", MOVE_SOURCES, bullets(MOVE_RISK, &gap("ask Mira Holt to label the boxes"), MOVE_DECISION), "pr1-bullets-valid", &[], &["mira", "holt", "person"]),
        reject("pr1-bullets-proposed-signoff", MOVE_SOURCES, bullets(MOVE_RISK, &gap("have the site coordinator sign off the labelling before the move"), MOVE_DECISION), "pr1-bullets-valid", &[], &["sign", "approv", "authority"]),
        reject("pr1-bullets-contradicts-ban", MOVE_SOURCES, bullets(MOVE_RISK, &gap("hire temporary help to label the boxes"), MOVE_DECISION), "pr1-bullets-valid", &[], &["hire", "hiring", "temporary", "staff"]),
        reject("pr1-bullets-waives-condition", MOVE_SOURCES, bullets(MOVE_RISK, &gap("start moving the boxes that are already labelled while the rest are labelled"), MOVE_DECISION), "pr1-bullets-valid", &[], &["condition", "waive", "every box", "all boxes", "start moving"]),
        reject("pr1-bullets-unlabelled-fact", MOVE_SOURCES, bullets(MOVE_RISK, "- Gap: the move may start only after every box is labelled. The records team will label the boxes before packing starts.", MOVE_DECISION), "pr1-bullets-valid", &[], &["records team", "assign", "proposal", "fact"]),
        reject("pr1-bullets-false-presupposition", MOVE_SOURCES, bullets(MOVE_RISK, &gap("since the shelf survey is already signed off, clarify now who labels the boxes"), MOVE_DECISION), "pr1-bullets-valid", &[], &["shelf survey", "signed off", "sign-off", "presuppos"]),
        reject("pr1-bullets-wrong-premise", MOVE_SOURCES, bullets(MOVE_RISK, MOVE_GAP, &MOVE_DECISION.replacen("the records manager decides", "the site coordinator decides", 1)), "pr1-bullets-valid", &[], &["site coordinator", "records manager", "decides"]),
        reject("pr1-bullets-effect-claim", MOVE_SOURCES, bullets(&MOVE_RISK.replacen("before booking the lift.", "before booking the lift; this removes the risk of delay.", 1), MOVE_GAP, MOVE_DECISION), "pr1-bullets-valid", &[], &["removes", "effect", "delay", "achieve"]),
        accept("pr1-row-clarify-valid", MOVE_SOURCES, row("| Label the boxes | not stated; proposal: clarify who labels the boxes | not stated | not stated |")),
        accept("pr1-row-source-role-valid", MOVE_SOURCES, row("| Label the boxes | not stated; proposal: records team, which packs the boxes | not stated | not stated |")),
        reject("pr1-row-invented-role", MOVE_SOURCES, row(VOLUNTEER_ROW), "pr1-row-source-role-valid", &[1], &["volunteer", "role", "not named"]),
        reject("pr1-row-unlabelled-assignment", MOVE_SOURCES, row("| Label the boxes | Records team | not stated | not stated |"), "pr1-row-source-role-valid", &[1], &["records team", "assign", "proposal", "not named"]),
        reject("pr1-row-presupposed-completion", MOVE_SOURCES, row("| Book the goods lift | Site coordinator | not stated | not stated; proposal: keep the booking already made for the move |"), "pr2-row-booking-valid", &[1], &["already", "booking", "made", "presuppos"]),
        reject("pr1-row-value-mislabelled-calculated", MOVE_SOURCES, row("| Label the boxes (calculated: one working day, right after packing) | not stated | not stated | not stated |"), "pr2-row-open-valid", &[1], &["calculat", "working day", "duration", "right after"]),
        accept("pr1-punkter-gyldig", VERKSTED_SOURCES, format!("{VERKSTED_RISK}\n{VERKSTED_GAP}")),
        reject("pr1-punkter-oppdiktet-varighet", VERKSTED_SOURCES, punkter("sett av to arbeidsdager til merkingen"), "pr1-punkter-gyldig", &[], &["arbeidsdag", "working day", "workday", "duration", "varighet"]),
        reject("pr1-punkter-oppdiktet-person", VERKSTED_SOURCES, punkter("be Siri Aune merke delene"), "pr1-punkter-gyldig", &[], &["siri", "aune", "person"]),
        accept("pr1-rad-avklaring-gyldig", VERKSTED_SOURCES, rad("| Merk delene | ikke oppgitt; forslag: avklar hvem som merker delene | ikke oppgitt | ikke oppgitt |")),
        reject("pr1-rad-uten-forslagsmerke", VERKSTED_SOURCES, rad("| Merk delene | Verkstedteamet | ikke oppgitt | ikke oppgitt |"), "pr1-rad-avklaring-gyldig", &[1], &["verkstedteam", "workshop team", "assign", "forslag", "proposal"]),
    ];
    // Batched pair: 23 segments, so four batches (as the four-batch-dependency
    // cases); the proposal (index 22) relies on segment 1, which sits in
    // another batch. The padding headings are lettered, so the controls carry
    // no number the fixtures use.
    for (id, booker, valid) in [
        ("pr1-batched-reliance-valid", "The site coordinator books the goods lift.", true),
        // The proposal must fall with the unsupported fact it relies on.
        ("pr1-batched-reliance-unsupported", "The records team books the goods lift.", false),
    ] {
        let mut parts = vec!["# Move notes".to_owned(), booker.to_owned()];
        parts.extend((b'A'..=b'T').map(|letter| format!("## Review note {}", char::from(letter))));
        parts.push(MOVE_LAST.to_owned());
        let candidate = parts.join("\n\n");
        controls.push(if valid {
            accept(id, MOVE_SOURCES, candidate)
        } else {
            reject(id, MOVE_SOURCES, candidate, "pr1-batched-reliance-valid", &[1, 22], &["records team", "books", "goods lift", "site coordinator"])
        });
    }
    controls.extend([
        // pr2: accept twins the pr1 rejects lacked.
        accept("pr2-row-booking-valid", MOVE_SOURCES, row("| Book the goods lift | Site coordinator | not stated | not stated; proposal: done when the lift is booked for the move |")),
        accept("pr2-row-open-valid", MOVE_SOURCES, row("| Label the boxes | not stated | not stated | not stated |")),
        // A computed schedule, so the note allowing a chosen later date is
        // present. Dates for the signs, which have no source duration (D4 for
        // a single chosen date; a range or a period set aside is a duration).
        accept("pr2-sched-signs-open-valid", HALL_SOURCES, signs("not stated; proposal: ask how long making the signs takes")),
        reject("pr2-sched-signs-range", HALL_SOURCES, signs("not stated; proposal: make the signs on 10–11 March"), "pr2-sched-signs-open-valid", &[2], &["range", "duration", "10–11", "10-11", "two days"]),
        reject("pr2-sched-signs-spare-days", HALL_SOURCES, signs("not stated; proposal: set aside 10–11 March as spare days for the signs"), "pr2-sched-signs-open-valid", &[2], &["spare", "set aside", "duration", "10–11", "10-11"]),
        accept("pr2-sched-signs-chosen-date-valid", HALL_SOURCES, signs("proposal (chosen date): ready by 19 March")),
        // C7: the same chosen date under the document's calculated label.
        reject("pr2-sched-signs-date-labelled-calculated", HALL_SOURCES, signs("calculated lower bound: ready by 19 March"), "pr2-sched-signs-chosen-date-valid", &[2], &["calculat", "chosen", "derived", "label"]),
        // A chosen date for a task with a calculated lower bound: later is a
        // choice, earlier breaks the bound.
        accept("pr2-sched-cleaning-later-date-valid", HALL_SOURCES, cleaning(&format!("{later}11 March"))),
        reject("pr2-sched-cleaning-earlier-date", HALL_SOURCES, cleaning(&format!("{later}7 March")), "pr2-sched-cleaning-later-date-valid", &[1], &["7 march", "earlier", "before", "lower bound", "painting"]),
        // The same values in a column its header labels as proposals.
        accept("pr2-sched-header-chosen-date-valid", HALL_SOURCES, proposed_signs("ready by 19 March")),
        reject("pr2-sched-header-range", HALL_SOURCES, proposed_signs("10–11 March"), "pr2-sched-header-chosen-date-valid", &[2], &["range", "duration", "10–11", "10-11", "two days"]),
        reject("pr2-sched-header-spare-days", HALL_SOURCES, proposed_signs("10–11 March, set aside as spare days"), "pr2-sched-header-chosen-date-valid", &[2], &["spare", "set aside", "duration", "10–11", "10-11"]),
        // Someone else's proposal is a factual claim about the sources.
        accept("pr2-attributed-valid", HALL_SOURCES, reopening("not decided; the caretaker has proposed 20 March")),
        reject("pr2-attributed-wrong-proposer", HALL_SOURCES, reopening("not decided; the painters have proposed 20 March"), "pr2-attributed-valid", &[2], &["painters", "caretaker", "attribut"]),
        // A column header as the label, and a header that labels nothing.
        accept("pr2-header-owner-valid", MOVE_SOURCES, owner("records team, which packs the boxes")),
        reject("pr2-header-owner-person", MOVE_SOURCES, owner("Mira Holt"), "pr2-header-owner-valid", &[1], &["mira", "holt", "person"]),
        reject("pr2-header-owner-approver", MOVE_SOURCES, owner("records team, which packs the boxes and signs off the labelling"), "pr2-header-owner-valid", &[1], &["sign", "approv", "authority"]),
        reject("pr2-header-owner-duration", MOVE_SOURCES, owner("records team, which packs the boxes, in one working day"), "pr2-header-owner-valid", &[1], &["working day", "one day", "duration"]),
        reject("pr2-header-owner-presupposed", MOVE_SOURCES, owner("records team, which packs the boxes, as already agreed"), "pr2-header-owner-valid", &[1], &["agreed", "agreement", "already", "presuppos"]),
        reject("pr2-header-follow-up-unlabelled", MOVE_SOURCES, format!("{MOVE_FOLLOW_UP_TABLE}| Label the boxes | not stated | records team, which packs the boxes |"), "pr2-header-owner-valid", &[1], &["follow-up", "follow up", "assign", "proposal", "header"]),
        // Two labelled proposals, each unobjectionable alone, that together
        // form a cycle no source line rules out.
        accept("pr2-proposals-order-valid", MOVE_SOURCES, "- Proposal: book the goods lift only after every box is labelled.\n\n- Proposal: label the boxes before the goods lift is booked.".to_owned()),
        reject("pr2-proposals-cycle", MOVE_SOURCES, "- Proposal: book the goods lift only after every box is labelled.\n\n- Proposal: label the boxes only after the goods lift is booked.".to_owned(), "pr2-proposals-order-valid", &[], &["circular", "cycle", "deadlock", "conflict", "contradict", "each other"]),
        // Other label forms (D5 for a recommendation label).
        accept("pr2-suggestion-valid", MOVE_SOURCES, bullets(MOVE_RISK, &MOVE_GAP.replacen("Proposal:", "Suggestion:", 1), MOVE_DECISION)),
        reject("pr2-suggestion-person", MOVE_SOURCES, bullets(MOVE_RISK, &MOVE_GAP.replacen("Proposal: clarify who labels the boxes before packing starts", "Suggestion: ask Mira Holt to label the boxes", 1), MOVE_DECISION), "pr2-suggestion-valid", &[], &["mira", "holt", "person"]),
        accept("pr2-anbefalt-gyldig", VERKSTED_SOURCES, format!("{VERKSTED_RISK}\n{}", VERKSTED_GAP.replacen("Forslag:", "Anbefalt:", 1))),
        reject("pr2-anbefalt-oppdiktet-varighet", VERKSTED_SOURCES, format!("{VERKSTED_RISK}\n{}", VERKSTED_GAP.replacen("Forslag: avklar hvem som merker delene før demonteringen starter", "Anbefalt: sett av to arbeidsdager til merkingen", 1)), "pr2-anbefalt-gyldig", &[], &["arbeidsdag", "working day", "workday", "duration", "varighet"]),
        // A heading or a lead-in line labels nothing.
        accept("pr2-heading-valid", MOVE_SOURCES, "## Proposals\n\n- Proposal: the records team, which packs the boxes, labels them.".to_owned()),
        reject("pr2-heading-unlabelled", MOVE_SOURCES, "## Proposals\n\n- The records team, which packs the boxes, labels them.".to_owned(), "pr2-heading-valid", &[1], &["heading", "assign", "proposal", "fact"]),
        accept("pr2-lead-in-valid", MOVE_SOURCES, "Proposals for the open points:\n- Proposal: the records team, which packs the boxes, labels them.".to_owned()),
        reject("pr2-lead-in-unlabelled", MOVE_SOURCES, "Proposals for the open points:\n- The records team, which packs the boxes, labels them.".to_owned(), "pr2-lead-in-valid", &[], &["lead-in", "introduct", "assign", "proposal", "fact"]),
        // A decision time limit, and a number that is not a duration.
        reject("pr2-bullets-decision-deadline", MOVE_SOURCES, bullets(MOVE_RISK, MOVE_GAP, &MOVE_DECISION.replacen("record the decision when it is taken", "have the records manager decide the move date by 6 March", 1)), "pr1-bullets-valid", &[], &["deadline", "march", "time limit"]),
        reject("pr2-bullets-invented-quantity", MOVE_SOURCES, bullets(MOVE_RISK, &gap("have three people label the boxes"), MOVE_DECISION), "pr1-bullets-valid", &[], &["three", "number", "quantity", "people"]),
        // A proposal alone in its segment: the no_assertions path.
        accept("pr2-standalone-valid", MOVE_SOURCES, "# Next steps\n\nProposal: agree on how the boxes will be labelled.".to_owned()),
        reject("pr2-standalone-duration", MOVE_SOURCES, "# Next steps\n\nProposal: set aside one working day to agree on how the boxes will be labelled.".to_owned(), "pr2-standalone-valid", &[1], &["working day", "one day", "duration"]),
        // Implicit presuppositions and effects: continuing something the
        // sources never mention, and a purpose clause that claims an outcome.
        reject("pr2-bullets-continue-scheme", MOVE_SOURCES, bullets(MOVE_RISK, &gap("continue using the current labelling scheme for the boxes"), MOVE_DECISION), "pr1-bullets-keep-condition-valid", &[], &["scheme", "current", "exist", "presuppos"]),
        reject("pr2-punkter-behold-merkeliste", VERKSTED_SOURCES, punkter("behold merkelisten fra forrige flytting"), "pr1-punkter-gyldig", &[], &["merkelist", "list", "forrige", "previous", "exist", "presuppos"]),
        accept("pr2-bullets-purpose-valid", MOVE_SOURCES, bullets(MOVE_RISK, &gap("clarify who labels the boxes, because the move may start only after every box is labelled"), MOVE_DECISION)),
        reject("pr2-bullets-purpose-effect", MOVE_SOURCES, bullets(MOVE_RISK, &gap("clarify who labels the boxes, so that the move is not delayed"), MOVE_DECISION), "pr2-bullets-purpose-valid", &[], &["delay", "effect", "achieve", "outcome", "so that"]),
        accept("pr2-punkter-formal-gyldig", VERKSTED_SOURCES, punkter("avklar hvem som merker delene, fordi flyttingen først kan starte når alle deler er merket")),
        reject("pr2-punkter-formal-effekt", VERKSTED_SOURCES, punkter("avklar hvem som merker delene, slik at flyttingen ikke blir forsinket"), "pr2-punkter-formal-gyldig", &[], &["forsink", "delay", "effect", "achieve", "slik at", "so that"]),
        // A role the sources do not name, allowed only when proposed roles are
        // requested (D3): by a brief the task adopts, or by the request itself.
        requested(ADOPTS_BRIEF, accept("pr2-brief-adopted-role-valid", BRIEF_SOURCES, row(VOLUNTEER_ROW))),
        requested(ADOPTS_BRIEF, reject("pr2-brief-adopted-person", BRIEF_SOURCES, row(&VOLUNTEER_ROW.replacen("a volunteer group from the building", "Mira Holt, a volunteer from the building", 1)), "pr2-brief-adopted-role-valid", &[1], &["mira", "holt", "person"])),
        requested(ADOPTS_BRIEF, reject("pr2-brief-adopted-approver", BRIEF_SOURCES, row(&VOLUNTEER_ROW.replacen("a volunteer group from the building", "a volunteer group from the building, which also signs off the labelling", 1)), "pr2-brief-adopted-role-valid", &[1], &["sign", "approv", "authority"])),
        requested(IGNORES_BRIEF, reject("pr2-brief-not-adopted", BRIEF_SOURCES, row(VOLUNTEER_ROW), "pr2-brief-adopted-role-valid", &[1], &["brief", "adopt", "volunteer", "not named"])),
        requested(ASKS_PROPOSED_OWNER, accept("pr2-request-proposed-owner-valid", MOVE_SOURCES, row(VOLUNTEER_ROW))),
        requested(ASKS_RESPONSIBLE_ROLE, reject("pr2-request-responsible-role", MOVE_SOURCES, row(VOLUNTEER_ROW), "pr2-request-proposed-owner-valid", &[1], &["volunteer", "not named", "request", "does not name"])),
    ]);
    controls
}

/// The recheck controls, as `tests/source_recheck_live.rs` plays them:
/// (id, source, faulty, dependent, replacement, expected, independent). The
/// dependent proposal contains a reference word, so it always gets a fresh
/// check; a rejection must reject it (index 2).
pub const RECHECK_CONTROLS: [(&str, &str, &str, &str, &str, bool, bool); 2] = [
    (
        "proposal-reference-preserved",
        "The North office's facilities coordinator books the meeting rooms. The South office has no facilities coordinator.",
        "The North office's facilities coordinator books the meeting rooms and orders lunch.",
        "Proposal: ask this coordinator which meeting rooms are free.",
        "The North office's facilities coordinator books the meeting rooms.",
        true,
        false,
    ),
    (
        "proposal-reference-changed",
        "The North office's facilities coordinator books the meeting rooms. The South office has no facilities coordinator.",
        "The North office's facilities coordinator books the meeting rooms and orders lunch.",
        "Proposal: ask this coordinator which meeting rooms are free.",
        "The South office has no facilities coordinator.",
        false,
        false,
    ),
];

/// Recheck controls whose scripted private repair of the rejected segment
/// (index 1) only adds a proposal label, or truly corrects it, as
/// `tests/source_recheck_live.rs` plays them: (id, source, faulty, dependent,
/// replacement, expected). The recheck sees neither the earlier wording nor
/// the reason it was rejected, so only the rule's reject list can stop a
/// relabelled claim; a rejection must reject the relabelled segment itself.
pub const RELABEL_CONTROLS: [(&str, &str, &str, &str, &str, bool); 4] = [
    (
        "proposal-relabel-signoff-corrected",
        MOVE,
        "The site coordinator signs off the labelling.",
        "The move may start only after every box is labelled and the goods lift is booked.",
        "The site coordinator books the goods lift.",
        true,
    ),
    (
        "proposal-relabel-signoff",
        MOVE,
        "The site coordinator signs off the labelling.",
        "The move may start only after every box is labelled and the goods lift is booked.",
        "Proposal: the site coordinator signs off the labelling.",
        false,
    ),
    (
        "proposal-relabel-duration-corrected",
        MOVE,
        "Labelling the boxes takes one working day.",
        "The move may start only after every box is labelled and the goods lift is booked.",
        "No one is named to label the boxes.",
        true,
    ),
    (
        "proposal-relabel-duration",
        MOVE,
        "Labelling the boxes takes one working day.",
        "The move may start only after every box is labelled and the goods lift is booked.",
        "Proposal: labelling the boxes takes one working day.",
        false,
    ),
];

/// Whether any of `reasons` names the changed clause: contains one of
/// `terms`, ignoring case.
pub fn reason_names_change(reasons: &[String], terms: &[&str]) -> bool {
    reasons.iter().any(|reason| {
        let reason = reason.to_lowercase();
        terms.iter().any(|term| reason.contains(&term.to_lowercase()))
    })
}

/// The reasons of every unsupported check in `responses`, the recorded model
/// replies (`{"content": …}`), in either wire format.
pub fn rejection_reasons(responses: &[Value]) -> Vec<String> {
    responses
        .iter()
        .filter_map(|response| serde_json::from_str::<Value>(response["content"].as_str()?).ok())
        .flat_map(|report| report["checks"].as_array().cloned().unwrap_or_default())
        .filter(|check| {
            matches!(
                check.get("v").or_else(|| check.get("status")).and_then(Value::as_str),
                Some("u" | "unsupported")
            )
        })
        .filter_map(|check| {
            check
                .get("r")
                .or_else(|| check.get("reason"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// Adds the twin-conditional result to every row of one run that names a
/// `twin`: `twinAccepted` (null when the twin did not run in this run) and
/// `evidence`, true only when the control was correctly rejected, its accept
/// twin was accepted in the same run and a rejection reason names the changed
/// clause (`reasonNamesChange`). Null evidence means the run cannot tell.
pub fn annotate_twins(rows: &mut [Value]) {
    let accepted: std::collections::BTreeMap<String, Value> = rows
        .iter()
        .filter_map(|row| Some((row["id"].as_str()?.to_owned(), row["accepted"].clone())))
        .collect();
    for row in rows.iter_mut() {
        let Some(twin) = row["twin"].as_str() else {
            continue;
        };
        let twin_accepted = accepted.get(twin).map(|accepted| accepted == &Value::Bool(true));
        let evidence = twin_accepted.map(|twin_accepted| {
            twin_accepted && row["correct"] == true && row["reasonNamesChange"] == true
        });
        row["twinAccepted"] = twin_accepted.map_or(Value::Null, Value::Bool);
        row["evidence"] = evidence.map_or(Value::Null, Value::Bool);
    }
}

/// The ids of the rows that fail a run: a row with confirmed labels whose
/// verdict is wrong, or whose twin-conditional evidence is false. Rows with
/// proposed labels are reported, never counted, so a run's exit status keeps
/// the confirmed bar in both rule states.
pub fn gating_failures(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .filter(|row| row["labelStatus"] == "confirmed")
        .filter(|row| row["correct"] != true || row["evidence"] == false)
        .map(|row| row["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}
