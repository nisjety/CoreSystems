//! Phase 5 — browser-action risk classification.
//!
//! Pure, unit-testable logic split out of `browser_agent.rs` to keep that
//! module's size in check: the types and the deterministic classifier here
//! have no dependency on `AgentPlan`/loop state — they only read
//! `BrowserAction`/`BrowserObservation`. The loop-integration glue that DOES
//! need `AgentPlan` (`gate_risky_action`, `gate_persistent_cookie_use`,
//! `request_approval`) stays in `browser_agent.rs` alongside `decide_next_action`.

use super::{extract_host, ActionType, BrowserAction, BrowserObservation, BrowserSnapshotTarget};

/// Category of elevated-risk browser action or run-level condition requiring
/// human approval before it proceeds (Phase 5 — HITL gates, plan capability
/// #8: "login, checkout, posting forms, destructive actions, downloads,
/// uploads, cross-domain navigation, persistent cookie use").
///
/// Deliberately does NOT cover `downloads`/`uploads`: no `ActionType`
/// variant (or Quarry-side manual-browsing capability) represents a file
/// transfer today — plan target capability #10 ("File upload/download
/// handling") is not yet built anywhere in the stack. Classifying risk for
/// an action that cannot occur would be a fabricated classifier, not a real
/// gate; this is an explicit, honest scope cut for this stage, not an
/// oversight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskCategory {
    /// The action interacts with a login/authentication form.
    Login,
    /// The action interacts with a checkout/payment flow.
    Checkout,
    /// The action submits/posts a form (comment, message, publish, …).
    PostingForm,
    /// The action looks destructive/irreversible (delete, remove, cancel,
    /// unsubscribe, deactivate, …).
    Destructive,
    /// A navigation whose target host differs from the last observed page's
    /// host. Distinct from — and additive to — the unconditional
    /// `allowed_domains` allow-list enforced at dispatch: a domain can be on
    /// the allow-list and still represent a meaningful hop worth a human's
    /// attention (e.g. moving from a shopping site to a third-party payment
    /// processor it redirects to).
    CrossDomainNavigation,
    /// The run reuses a persistent, cookie-bearing Quarry browser profile
    /// (not ZDR, not ephemeral). Gated once per run rather than per action.
    PersistentCookieUse,
}

impl RiskCategory {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Checkout => "checkout",
            Self::PostingForm => "posting_form",
            Self::Destructive => "destructive",
            Self::CrossDomainNavigation => "cross_domain_navigation",
            Self::PersistentCookieUse => "persistent_cookie_use",
        }
    }

    /// Parse a planner- or caller-supplied risk-category string (Phase 5's
    /// `ACTION_SCHEMA.risk_category`). Unknown, empty, or `"none"` values are
    /// not an error — they just mean "no self-reported risk"; the
    /// deterministic backstop (`classify_action_risk`) still runs regardless.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "login" => Some(Self::Login),
            "checkout" => Some(Self::Checkout),
            "posting_form" | "post" | "form_submit" | "posting form" => Some(Self::PostingForm),
            "destructive" => Some(Self::Destructive),
            "cross_domain" | "cross_domain_navigation" | "cross-domain" => {
                Some(Self::CrossDomainNavigation)
            }
            "persistent_cookie" | "persistent_cookie_use" | "persistent cookie" => {
                Some(Self::PersistentCookieUse)
            }
            _ => None,
        }
    }
}

/// Detail describing WHICH browser action (or run-level condition) triggered
/// the Phase 5 HITL approval gate, and WHY — carried onto the
/// `BrowserActionApprovalRequired`/`BrowserActionDecided` orchestration
/// events so the run timeline shows a specific reason instead of a generic
/// "risky tool" label (contrast the outer, whole-tool `is_risky_tool` gate).
#[derive(Debug, Clone)]
pub struct RiskyActionDetail {
    /// Empty for a run-level gate (e.g. `PersistentCookieUse`) not tied to
    /// one specific dispatched action.
    pub action_id: String,
    /// Action type slug, or a synthetic slug (e.g. `"start_run"`) for a
    /// run-level gate.
    pub action_type: String,
    pub url: String,
    pub selector: String,
    pub risk_category: RiskCategory,
    pub reason: String,
}

/// Outcome of the Phase 5 in-loop HITL approval gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    /// A human granted the approval — proceed with the gated action/run.
    Granted,
    /// A human denied it, or the gate could not be reached at all (fail
    /// closed — see `BrowserEventSink::require_approval`'s doc).
    Denied(String),
    /// No decision arrived within the wait budget.
    TimedOut,
}

// Keyword/URL-path backstop marker lists for categories with no structural
// signal in `BrowserAction` (mirrors `permission::is_risky_tool`'s
// keyword-list pattern). Checked in a fixed, most-specific-first order (see
// `classify_deterministic_risk` below) so an action touching more than one
// marker set (e.g. a checkout page's own login gate) still gets a single,
// sensible classification.
//
// The lists cover English and Norwegian. Every marker is looked for in every
// piece of evidence (URL, selector, the label of a bound snapshot target), so
// each one has to be safe in running text, not only in a URL: a bare "cart" or
// "cancel" would gate "Add to cart" and every dialog's dismiss button.

/// How a marker has to line up with the evidence it is looked for in. Markers
/// are lower-case; the evidence is lower-cased before the comparison.
#[derive(Debug, Clone, Copy)]
enum Marker {
    /// A fragment anywhere in the text, across word boundaries. For URL paths
    /// ("/checkout"), compounds ("buynow") and stems that are never part of a
    /// different word ("password", "passord").
    Anywhere(&'static str),
    /// An English verb that names a control ("delete", "submit"). Found inside
    /// a longer word too ("deleteaccount", "btndelete"), but not as a bare
    /// inflection of itself: "removed", "published" and "submits" describe a
    /// state or a noun, they are not a control.
    Verb(&'static str),
    /// A whole word, or a run of whole words ("logg inn"), after the text is
    /// split at punctuation, camelCase and letter/digit boundaries: "meld-av",
    /// "meldAv" and "Meld av" all hold "meld av", and "sendt" does not hold
    /// "send". Norwegian markers are spelled out in the forms a control uses
    /// (imperative and infinitive) instead of matched by stem, because "kasse"
    /// is also "kassett" and "slett" is also "slettet".
    Word(&'static str),
}

/// What may follow a [`Marker::Verb`] in the same word for that word to be a
/// form of the verb ("removed", "stain remover", "published", "submitting")
/// rather than something that names a control ("removefromcart").
const VERB_INFLECTIONS: &[&str] = &[
    "d", "ed", "ted", "s", "es", "r", "rs", "er", "ers", "ter", "ters", "ing", "ting",
];

const CHECKOUT_MARKERS: &[Marker] = &[
    Marker::Anywhere("/checkout"),
    Marker::Anywhere("/cart"),
    Marker::Anywhere("/payment"),
    Marker::Anywhere("/billing"),
    Marker::Anywhere("/handlekurv"),
    Marker::Anywhere("place-order"),
    Marker::Anywhere("placeorder"),
    Marker::Anywhere("buy-now"),
    Marker::Anywhere("buynow"),
    Marker::Anywhere("pay-now"),
    Marker::Anywhere("paynow"),
    Marker::Word("checkout"),
    Marker::Word("payment"),
    Marker::Word("place order"),
    Marker::Word("buy now"),
    Marker::Word("pay now"),
    Marker::Word("kasse"),
    Marker::Word("kassen"),
    Marker::Word("betaling"),
    Marker::Word("betalingen"),
    Marker::Word("betal"),
    Marker::Word("betale"),
    Marker::Word("bestill"),
    Marker::Word("bestille"),
    Marker::Word("fullfør bestilling"),
    Marker::Word("fullfor bestilling"),
    Marker::Word("legg inn bestilling"),
    Marker::Word("send bestilling"),
    Marker::Word("kjøp nå"),
    Marker::Word("kjop na"),
];
const LOGIN_MARKERS: &[Marker] = &[
    Marker::Anywhere("/login"),
    Marker::Anywhere("/signin"),
    Marker::Anywhere("/sign-in"),
    Marker::Anywhere("password"),
    Marker::Anywhere("passwd"),
    Marker::Anywhere("pwd"),
    Marker::Anywhere("passord"),
    Marker::Anywhere("innlogging"),
    Marker::Word("login"),
    Marker::Word("log in"),
    Marker::Word("signin"),
    Marker::Word("sign in"),
    Marker::Word("logginn"),
    Marker::Word("logg inn"),
    Marker::Word("logg på"),
    Marker::Word("logg pa"),
];
const DESTRUCTIVE_MARKERS: &[Marker] = &[
    Marker::Verb("delete"),
    Marker::Verb("remove"),
    Marker::Verb("unsubscribe"),
    Marker::Verb("deactivate"),
    Marker::Anywhere("cancel-order"),
    Marker::Anywhere("cancelorder"),
    Marker::Anywhere("close-account"),
    Marker::Anywhere("closeaccount"),
    Marker::Word("cancel order"),
    Marker::Word("close account"),
    Marker::Word("slett"),
    Marker::Word("slette"),
    Marker::Word("fjern"),
    Marker::Word("fjerne"),
    Marker::Word("avbestill"),
    Marker::Word("avbestille"),
    Marker::Word("kanseller ordre"),
    Marker::Word("kanseller bestilling"),
    Marker::Word("avmeld"),
    Marker::Word("avmelde"),
    Marker::Word("meld av"),
    Marker::Word("meld deg av"),
    Marker::Word("deaktiver"),
    Marker::Word("deaktivere"),
    Marker::Word("lukk konto"),
];
const POSTING_MARKERS: &[Marker] = &[
    Marker::Verb("submit"),
    Marker::Verb("publish"),
    Marker::Anywhere("post-comment"),
    Marker::Anywhere("send-message"),
    Marker::Word("post comment"),
    Marker::Word("send"),
    Marker::Word("publiser"),
    Marker::Word("publisere"),
];

/// Deterministic risk classification with the planner's self-report as an
/// additive fallback. Concrete browser evidence wins whenever both are
/// present, so a planner cannot relabel a checkout or destructive action to
/// make the human approval prompt less informative. Never classifies
/// `downloads`/`uploads` — see [`RiskCategory`]'s doc for why. What counts as
/// concrete evidence, and what does not, is described on `Evidence`.
///
/// Heuristic, and the planner reads untrusted page text, so a page can label a
/// control to dodge it: the domain fence in `AgentPlan` stays the hard guard.
pub(super) fn classify_action_risk(
    action: &BrowserAction,
    last_observation: Option<&BrowserObservation>,
) -> Option<RiskCategory> {
    let deterministic = classify_deterministic_risk(action, last_observation);
    deterministic.or(action.risk_category)
}

fn classify_deterministic_risk(
    action: &BrowserAction,
    last_observation: Option<&BrowserObservation>,
) -> Option<RiskCategory> {
    // Cross-domain navigation: compare the target host of a `Goto` against
    // the host of the last page actually observed — NOT the allow-list
    // (`AgentPlan::is_domain_allowed`), which is a separate, unconditional
    // dispatch-time check that stays unchanged. A page can legitimately
    // redirect across allow-listed domains (e.g. a shop to its payment
    // processor); that hop is still worth a human's attention. The very
    // first navigation of a run (no prior observation yet) is never flagged
    // here — there is no "prior domain" to have moved away from.
    if action.action_type == ActionType::Goto {
        if let (Some(target_host), Some(prior_host)) = (
            extract_host(&action.url),
            last_observation.and_then(|o| extract_host(&o.page_url)),
        ) {
            if target_host != prior_host {
                return Some(RiskCategory::CrossDomainNavigation);
            }
        }
    }

    let evidence = Evidence::of(action, last_observation);

    if evidence.holds_any(CHECKOUT_MARKERS) {
        return Some(RiskCategory::Checkout);
    }
    if evidence.holds_any(LOGIN_MARKERS) {
        return Some(RiskCategory::Login);
    }
    if evidence.holds_any(DESTRUCTIVE_MARKERS) {
        return Some(RiskCategory::Destructive);
    }
    if matches!(action.action_type, ActionType::Click) && evidence.holds_any(POSTING_MARKERS) {
        return Some(RiskCategory::PostingForm);
    }

    None
}

/// The text a classification is judged on, normalised once. It is what the
/// action will act on and nothing else:
///
/// - the URL and the selector, percent-decoded so `/log%69n` reads as
///   `/login`, the way the server will see it;
/// - for a click or a type bound to a snapshot ref, the role and name of the
///   target that ref names in the last observation, and for a click its
///   visible text too. The planner is told to prefer the ref and to fall back
///   to a selector only when no snapshot target fits, so a ref-bound action
///   usually has an empty selector and the target is the only concrete
///   evidence there is: without it a "Slett konto" button, or a field labelled
///   "Passord", would pass the gate unseen.
///
/// Typed text (`action.value`) is not read. It is content, not a description
/// of the control, and matching prose by keyword asked for approval on a
/// search for "stain remover" and on a comment that said an item "was
/// removed". Page text, page title and the planner's own `reason` are not read
/// either: they are untrusted prose, and the planner's say is only the
/// fallback in [`classify_action_risk`].
struct Evidence {
    texts: Vec<EvidenceText>,
}

impl Evidence {
    fn of(action: &BrowserAction, last_observation: Option<&BrowserObservation>) -> Self {
        let mut texts = vec![
            EvidenceText::new(&percent_decode(&action.url)),
            EvidenceText::new(&percent_decode(&action.selector)),
        ];
        if let Some(target) = bound_target(action, last_observation) {
            texts.extend(target.role.as_deref().map(EvidenceText::new));
            texts.extend(target.name.as_deref().map(EvidenceText::new));
            // A field's `text` is its current content (Quarry falls back to
            // the field's value when it has no name), which is user data. A
            // link or button's `text` is its label.
            if action.action_type == ActionType::Click {
                texts.extend(target.text.as_deref().map(EvidenceText::new));
            }
        }
        Self { texts }
    }

    fn holds_any(&self, markers: &[Marker]) -> bool {
        markers
            .iter()
            .any(|&marker| self.texts.iter().any(|text| text.holds(marker)))
    }
}

/// The snapshot target a click or type is bound to. The ref is looked up in
/// the last observation by its id alone: ids are unique within a snapshot, and
/// Quarry refuses a ref from another generation, but the gate must not lean on
/// that refusal, so a ref naming `@e3` is judged by the `@e3` the planner was
/// shown. Other actions do not operate a target, so they have none.
fn bound_target<'a>(
    action: &BrowserAction,
    last_observation: Option<&'a BrowserObservation>,
) -> Option<&'a BrowserSnapshotTarget> {
    if !matches!(action.action_type, ActionType::Click | ActionType::Type) {
        return None;
    }
    let bound = action.target.as_ref()?;
    last_observation?
        .snapshot_targets
        .iter()
        .find(|target| target.ref_id == bound.ref_id)
}

/// One piece of evidence in the forms the three [`Marker`] kinds compare against.
struct EvidenceText {
    lower: String,
    words: Vec<String>,
    /// `words` joined by single spaces, with a space on each side, so a run of
    /// whole words is a plain substring search for " run of words ".
    padded: String,
}

impl EvidenceText {
    fn new(text: &str) -> Self {
        let words = split_words(text);
        let padded = format!(" {} ", words.join(" "));
        Self {
            lower: text.to_lowercase(),
            words,
            padded,
        }
    }

    fn holds(&self, marker: Marker) -> bool {
        match marker {
            Marker::Anywhere(fragment) => self.lower.contains(fragment),
            Marker::Verb(verb) => self.words.iter().any(|word| word_holds_verb(word, verb)),
            Marker::Word(phrase) => self.padded.contains(&format!(" {phrase} ")),
        }
    }
}

fn word_holds_verb(word: &str, verb: &str) -> bool {
    word.match_indices(verb)
        .any(|(at, _)| !VERB_INFLECTIONS.contains(&&word[at + verb.len()..]))
}

/// The lower-cased words of `text`. A word ends at anything that is not a
/// letter or a digit, at a camelCase hump (`btnDelete` is "btn", "delete";
/// `HTMLParser` is "html", "parser") and between letters and digits (`step2`
/// is "step", "2").
fn split_words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        if let Some(&prev) = i.checked_sub(1).and_then(|p| chars.get(p)) {
            let next_is_lower = chars.get(i + 1).is_some_and(|next| next.is_lowercase());
            let boundary = prev.is_alphanumeric()
                && ((prev.is_lowercase() && c.is_uppercase())
                    || (prev.is_uppercase() && c.is_uppercase() && next_is_lower)
                    || prev.is_alphabetic() != c.is_alphabetic());
            if boundary && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
        word.extend(c.to_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Decode `%XX` escapes (UTF-8, lossy) so an escaped marker such as `/log%69n`
/// is read as what the server will see. Text without a valid escape comes back
/// unchanged.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(byte) = bytes.get(i + 1..i + 3).and_then(hex_byte) {
                decoded.push(byte);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_byte(digits: &[u8]) -> Option<u8> {
    let &[high, low] = digits else {
        return None;
    };
    let digit = |b: u8| char::from(b).to_digit(16);
    u8::try_from(digit(high)? * 16 + digit(low)?).ok()
}

/// Human-readable "why" for a per-action gate, combining the classification
/// with the model's own rationale (`action.reason`) when present.
pub(super) fn risk_reason(category: RiskCategory, action: &BrowserAction) -> String {
    let base = match category {
        RiskCategory::CrossDomainNavigation => {
            format!("navigation to a new domain: {}", action.url)
        }
        RiskCategory::Login => {
            "action appears to interact with a login/authentication form".to_owned()
        }
        RiskCategory::Checkout => {
            "action appears to interact with a checkout/payment flow".to_owned()
        }
        RiskCategory::PostingForm => "action appears to submit/post a form".to_owned(),
        RiskCategory::Destructive => {
            "action appears destructive (delete/remove/cancel/unsubscribe/deactivate)".to_owned()
        }
        RiskCategory::PersistentCookieUse => {
            "run reuses a persistent cookie-bearing profile".to_owned()
        }
    };
    if action.reason.is_empty() {
        base
    } else {
        format!("{base} (model: {})", action.reason)
    }
}

/// Polling interval while waiting on a Phase 5 HITL approval decision.
/// Shortened under test, mirrors `super::pause_poll_interval`.
pub(crate) fn approval_poll_interval() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(5)
    } else {
        std::time::Duration::from_millis(500)
    }
}

/// How long (seconds) to wait for a human decision before a risky browser
/// action's approval gate gives up and treats it as timed out (fail closed —
/// never `Granted` by default). Overridable via
/// `QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S` for ops; short under test so the
/// unit suite never blocks on real wall-clock time.
pub(crate) fn approval_timeout_seconds() -> u32 {
    if cfg!(test) {
        return 1;
    }
    std::env::var("QUARRY_BROWSER_AGENT_APPROVAL_TIMEOUT_S")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(1800)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_agent::{BrowserSnapshotTarget, BrowserTargetRef, ObservationStatus};

    fn action(action_type: ActionType, url: &str, selector: &str, value: &str) -> BrowserAction {
        BrowserAction {
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            action_type,
            selector: selector.to_owned(),
            value: value.to_owned(),
            url: url.to_owned(),
            max_wait_ms: 5000,
            target: None,
            reason: String::new(),
            risk_category: None,
        }
    }

    fn observation_at(url: &str) -> BrowserObservation {
        BrowserObservation {
            observation_id: "obs_1".to_owned(),
            action_id: "act_0".to_owned(),
            grant_id: "grant_1".to_owned(),
            status: ObservationStatus::Success,
            page_url: url.to_owned(),
            page_title: String::new(),
            extracted_text: String::new(),
            screenshot_ref: String::new(),
            dom_snapshot_ref: String::new(),
            snapshot_generation: None,
            snapshot_targets: vec![],
            error_message: String::new(),
        }
    }

    #[test]
    fn classify_planner_self_report_covers_a_risk_without_deterministic_evidence() {
        let mut a = action(
            ActionType::Click,
            "https://example.com/help",
            "button.info",
            "",
        );
        a.risk_category = Some(RiskCategory::Destructive);
        assert_eq!(
            classify_action_risk(&a, None),
            Some(RiskCategory::Destructive)
        );
    }

    #[test]
    fn classify_deterministic_checkout_is_not_masked_by_a_planner_login_label() {
        let mut a = action(
            ActionType::Click,
            "https://shop.example.com/checkout",
            "button.continue",
            "",
        );
        a.risk_category = Some(RiskCategory::Login);

        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Checkout));
    }

    #[test]
    fn classify_detects_checkout_by_url() {
        let a = action(
            ActionType::Goto,
            "https://shop.example.com/checkout",
            "",
            "",
        );
        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Checkout));
    }

    #[test]
    fn classify_detects_login_by_selector() {
        let a = action(
            ActionType::Type,
            "https://example.com/account",
            "input[type=password]",
            "hunter2",
        );
        assert_eq!(classify_action_risk(&a, None), Some(RiskCategory::Login));
    }

    #[test]
    fn classify_detects_destructive_by_selector() {
        let a = action(
            ActionType::Click,
            "https://example.com/settings",
            "button.delete-account",
            "",
        );
        assert_eq!(
            classify_action_risk(&a, None),
            Some(RiskCategory::Destructive)
        );
    }

    #[test]
    fn classify_detects_posting_form_on_click_only() {
        let click = action(
            ActionType::Click,
            "https://example.com/comments",
            "button[type=submit]",
            "",
        );
        assert_eq!(
            classify_action_risk(&click, None),
            Some(RiskCategory::PostingForm)
        );

        // Same marker on a non-click action type doesn't count (no submit
        // affordance actually being invoked).
        let scroll = action(
            ActionType::Scroll,
            "https://example.com/comments",
            "button[type=submit]",
            "",
        );
        assert_eq!(classify_action_risk(&scroll, None), None);
    }

    #[test]
    fn classify_detects_cross_domain_navigation_relative_to_last_observation() {
        let goto = action(
            ActionType::Goto,
            "https://alt-domain.example/landing",
            "",
            "",
        );
        let last = observation_at("https://shop.example.com/cart");
        assert_eq!(
            classify_action_risk(&goto, Some(&last)),
            Some(RiskCategory::CrossDomainNavigation)
        );
    }

    #[test]
    fn classify_does_not_flag_same_domain_navigation() {
        let goto = action(ActionType::Goto, "https://shop.example.com/about", "", "");
        let last = observation_at("https://shop.example.com/cart");
        assert_eq!(classify_action_risk(&goto, Some(&last)), None);
    }

    #[test]
    fn classify_does_not_flag_the_first_navigation_of_a_run() {
        // No prior observation yet — nothing to have moved "away" from.
        let goto = action(ActionType::Goto, "https://example.com/", "", "");
        assert_eq!(classify_action_risk(&goto, None), None);
    }

    #[test]
    fn classify_returns_none_for_a_benign_action() {
        let observe = action(ActionType::Observe, "", "", "");
        assert_eq!(classify_action_risk(&observe, None), None);
    }

    // ---- Regression table (task E7) -----------------------------------
    //
    // One table per theme, so a failing run lists every row that moved and
    // not just the first.

    const NONE: Option<RiskCategory> = None;
    const CHECKOUT: Option<RiskCategory> = Some(RiskCategory::Checkout);
    const LOGIN: Option<RiskCategory> = Some(RiskCategory::Login);
    const DESTRUCTIVE: Option<RiskCategory> = Some(RiskCategory::Destructive);
    const POSTING: Option<RiskCategory> = Some(RiskCategory::PostingForm);
    const CROSS_DOMAIN: Option<RiskCategory> = Some(RiskCategory::CrossDomainNavigation);

    /// One row: an action, the page it was chosen from, and the category the
    /// gate must pick for it.
    struct Row {
        name: String,
        action: BrowserAction,
        page: Option<BrowserObservation>,
        expect: Option<RiskCategory>,
    }

    fn row(
        name: impl Into<String>,
        action: BrowserAction,
        page: Option<BrowserObservation>,
        expect: Option<RiskCategory>,
    ) -> Row {
        Row {
            name: name.into(),
            action,
            page,
            expect,
        }
    }

    fn assert_table(rows: &[Row]) {
        let misses: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                let got = classify_action_risk(&row.action, row.page.as_ref());
                (got != row.expect)
                    .then(|| format!("  {}: expected {:?}, got {got:?}", row.name, row.expect))
            })
            .collect();
        assert!(
            misses.is_empty(),
            "{} of {} rows misclassified:\n{}",
            misses.len(),
            rows.len(),
            misses.join("\n")
        );
    }

    fn snapshot_target(
        ref_id: &str,
        role: &str,
        name: Option<&str>,
        text: Option<&str>,
    ) -> BrowserSnapshotTarget {
        BrowserSnapshotTarget {
            ref_id: ref_id.to_owned(),
            role: Some(role.to_owned()),
            name: name.map(str::to_owned),
            text: text.map(str::to_owned),
            frame_id: None,
        }
    }

    fn page_with(targets: Vec<BrowserSnapshotTarget>) -> BrowserObservation {
        BrowserObservation {
            dom_snapshot_ref: "snap_1".to_owned(),
            snapshot_generation: Some(1),
            snapshot_targets: targets,
            ..observation_at("https://butikk.example.no/")
        }
    }

    /// Bind `action` to a snapshot ref the way the planner does when it
    /// echoes the ids from the observation (selector left empty).
    fn bound_to(mut action: BrowserAction, ref_id: &str) -> BrowserAction {
        action.target = Some(BrowserTargetRef {
            snapshot_id: "snap_1".to_owned(),
            generation: 1,
            ref_id: ref_id.to_owned(),
            frame_id: None,
        });
        action
    }

    /// A navigation to `url`, first of its run.
    fn goto_row(url: &str, expect: Option<RiskCategory>) -> Row {
        let step = action(ActionType::Goto, url, "", "");
        row(format!("goto {url}"), step, None, expect)
    }

    /// A click on a CSS selector, no snapshot target (the legacy form).
    fn click_row(selector: &str, expect: Option<RiskCategory>) -> Row {
        let step = action(ActionType::Click, "", selector, "");
        row(format!("click {selector}"), step, None, expect)
    }

    /// A click bound to the snapshot target `@e2`, whose visible label is
    /// `label`, next to a harmless decoy target.
    fn label_row(role: &str, label: &str, expect: Option<RiskCategory>) -> Row {
        let page = page_with(vec![
            snapshot_target("@e1", "link", Some("Hjem"), Some("Hjem")),
            snapshot_target("@e2", role, Some(label), Some(label)),
        ]);
        let step = bound_to(action(ActionType::Click, "", "", ""), "@e2");
        row(format!("click {role} {label:?}"), step, Some(page), expect)
    }

    fn button(label: &str, expect: Option<RiskCategory>) -> Row {
        label_row("button", label, expect)
    }

    fn link(label: &str, expect: Option<RiskCategory>) -> Row {
        label_row("link", label, expect)
    }

    /// Typing `value` into the snapshot target `@e2`, a text field whose
    /// accessible name is `field_name`.
    fn typed_row(field_name: Option<&str>, value: &str, expect: Option<RiskCategory>) -> Row {
        let page = page_with(vec![
            snapshot_target("@e1", "link", Some("Hjem"), Some("Hjem")),
            snapshot_target("@e2", "textbox", field_name, None),
        ]);
        let step = bound_to(action(ActionType::Type, "", "", value), "@e2");
        row(
            format!("type {value:?} into {field_name:?}"),
            step,
            Some(page),
            expect,
        )
    }

    #[test]
    fn norwegian_urls_are_classified() {
        assert_table(&[
            goto_row("https://butikk.example.no/kasse", CHECKOUT),
            goto_row("https://butikk.example.no/kassen/steg-2", CHECKOUT),
            goto_row("https://kasse.example.no/", CHECKOUT),
            goto_row("https://butikk.example.no/betaling", CHECKOUT),
            goto_row("https://butikk.example.no/betal-na", CHECKOUT),
            goto_row("https://butikk.example.no/bestill", CHECKOUT),
            goto_row("https://butikk.example.no/handlekurv", CHECKOUT),
            goto_row("https://min.example.no/logg-inn", LOGIN),
            goto_row("https://min.example.no/logginn", LOGIN),
            goto_row("https://min.example.no/innlogging", LOGIN),
            goto_row("https://min.example.no/innloggingsside", LOGIN),
            goto_row("https://min.example.no/glemt-passord", LOGIN),
            goto_row("https://min.example.no/min-side/slett-konto", DESTRUCTIVE),
            goto_row("https://min.example.no/avbestill/ordre-123", DESTRUCTIVE),
            goto_row("https://nyhetsbrev.example.no/meld-av", DESTRUCTIVE),
            goto_row("https://nyhetsbrev.example.no/avmeld", DESTRUCTIVE),
            // Escaped, the way a server decodes them.
            goto_row("https://example.no/logg%20inn", LOGIN),
            goto_row("https://example.no/kj%C3%B8p-n%C3%A5", CHECKOUT),
        ]);
    }

    #[test]
    fn norwegian_selectors_are_classified() {
        let password_input = row(
            "type into input#passord",
            action(ActionType::Type, "", "input#passord", "hemmelig"),
            None,
            LOGIN,
        );
        assert_table(&[
            click_row("button.slett-konto", DESTRUCTIVE),
            click_row("#btnSlett", DESTRUCTIVE),
            click_row("a.fjern-fra-handlekurv", DESTRUCTIVE),
            click_row("button.avbestill-ordre", DESTRUCTIVE),
            click_row("a.meld-av", DESTRUCTIVE),
            click_row("button.send-inn", POSTING),
            click_row("button.send", POSTING),
            click_row("button.publiser", POSTING),
            click_row("button.bestill-na", CHECKOUT),
            click_row("button.betal", CHECKOUT),
            click_row("button.logg-inn", LOGIN),
            password_input,
        ]);
    }

    #[test]
    fn labels_of_ref_bound_clicks_are_evidence() {
        // The planner is told to prefer an opaque snapshot ref over a
        // selector, so the label of the bound target is usually the only
        // concrete evidence a click carries.
        assert_table(&[
            link("Logg inn", LOGIN),
            button("Logg inn med BankID", LOGIN),
            button("Logg på", LOGIN),
            button("Gå til kassen", CHECKOUT),
            button("Betal nå", CHECKOUT),
            button("Fullfør bestilling", CHECKOUT),
            button("Kjøp nå", CHECKOUT),
            button("Slett konto", DESTRUCTIVE),
            button("Fjern", DESTRUCTIVE),
            button("Avbestill", DESTRUCTIVE),
            button("Meld deg av", DESTRUCTIVE),
            button("Send inn", POSTING),
            button("Send melding", POSTING),
            button("Publiser", POSTING),
            link("Sign in", LOGIN),
            button("Log in", LOGIN),
            button("Place order", CHECKOUT),
            button("Proceed to checkout", CHECKOUT),
            button("Delete account", DESTRUCTIVE),
            button("Remove", DESTRUCTIVE),
            button("Submit", POSTING),
        ]);
    }

    #[test]
    fn a_password_field_is_login_in_both_languages() {
        let by_role = row(
            "type into a field whose role is password and that has no name",
            bound_to(action(ActionType::Type, "", "", "x"), "@e2"),
            Some(page_with(vec![snapshot_target(
                "@e2", "password", None, None,
            )])),
            LOGIN,
        );
        assert_table(&[
            typed_row(Some("Password"), "hunter2", LOGIN),
            typed_row(Some("Passord"), "hemmelig", LOGIN),
            typed_row(Some("Gjenta passord"), "hemmelig", LOGIN),
            typed_row(Some("Passordet ditt"), "hemmelig", LOGIN),
            typed_row(Some("confirmPassword"), "hunter2", LOGIN),
            by_role,
            typed_row(Some("Brukernavn"), "ola", NONE),
            typed_row(Some("E-post"), "ola@example.no", NONE),
            typed_row(Some("Søk"), "kaffe", NONE),
            // Chromium's accessibility tree reports a password input as a
            // plain `textbox` and only the label tells it apart, so a field
            // with no label is invisible to the classifier. Pinned so a change
            // in what Quarry exposes is a visible decision.
            typed_row(None, "hunter2", NONE),
        ]);
    }

    #[test]
    fn typed_text_is_content_not_evidence() {
        assert_table(&[
            typed_row(
                Some("Notater"),
                "please delete my old notes, send the password later and log in",
                NONE,
            ),
            typed_row(
                Some("Kommentar"),
                "the item was removed from the list",
                NONE,
            ),
            typed_row(
                Some("Kommentar"),
                "jeg vil slette kontoen og betale i kassen",
                NONE,
            ),
        ]);
    }

    #[test]
    fn words_that_only_look_like_a_marker_do_not_trip() {
        assert_table(&[
            // Past participles, nouns and other forms of the verbs.
            link("Deleted items", NONE),
            link("Removed", NONE),
            link("Stain remover", NONE),
            button("Unsubscribed", NONE),
            link("Published", NONE),
            link("Publishers", NONE),
            link("Slettede elementer", NONE),
            link("Slettet", NONE),
            link("Konto slettet", NONE),
            link("Fjernet", NONE),
            link("Publisert", NONE),
            link("Sendt", NONE),
            link("Sender", NONE),
            link("Sending av pakke", NONE),
            link("Sent items", NONE),
            // Neighbouring, harmless controls.
            button("Cancel", NONE),
            button("Avbryt", NONE),
            button("Add to cart", NONE),
            button("Legg i handlekurv", NONE),
            link("Designing your garden", NONE),
            link("Mine bestillinger", NONE),
            goto_row("https://example.com/news/removed-from-index", NONE),
            goto_row("https://example.com/mail/deleted-items", NONE),
            goto_row("https://example.com/designing-your-garden", NONE),
            goto_row("https://example.com/about/publishers", NONE),
            goto_row("https://example.no/epost/sendt", NONE),
            goto_row("https://example.no/nyheter/slettet-fra-registeret", NONE),
            goto_row("https://butikk.example.no/mine-bestillinger", NONE),
            click_row("li.submitted", NONE),
            click_row("li.item-removed", NONE),
        ]);
    }

    #[test]
    fn page_text_and_title_are_not_evidence() {
        let mut page = page_with(vec![snapshot_target(
            "@e2",
            "link",
            Some("Les mer"),
            Some("Les mer"),
        )]);
        page.page_title = "Logg inn".to_owned();
        page.extracted_text =
            "Your account was deleted. Please log in to delete the rest. Betal nå i kassen."
                .to_owned();
        assert_table(&[row(
            "a harmless link on a page that talks about everything",
            bound_to(action(ActionType::Click, "", "", ""), "@e2"),
            Some(page),
            NONE,
        )]);
    }

    #[test]
    fn uploads_and_downloads_stay_unclassified() {
        // The loop has no upload or download action yet; see `RiskCategory`.
        assert_table(&[
            link("Download report", NONE),
            link("Last ned rapport", NONE),
            button("Upload file", NONE),
            button("Last opp fil", NONE),
            goto_row("https://example.com/files/r.pdf?download=1", NONE),
            goto_row("https://example.com/upload", NONE),
            click_row("a.download-link", NONE),
        ]);
    }

    #[test]
    fn english_markers_keep_their_recall() {
        assert_table(&[
            goto_row("https://example.com/login", LOGIN),
            goto_row("https://example.com/sign-in", LOGIN),
            goto_row("https://example.com/billing/cards", CHECKOUT),
            goto_row("https://example.com/cart", CHECKOUT),
            goto_row("https://example.com/deleteaccount", DESTRUCTIVE),
            goto_row(
                "https://shop.example.com/removefromcart?item=3",
                DESTRUCTIVE,
            ),
            goto_row("https://example.com/unsubscribe?token=abc", DESTRUCTIVE),
            goto_row("https://example.com/log%69n", LOGIN),
            click_row("#btnDeleteAccount", DESTRUCTIVE),
            click_row("#btndelete", DESTRUCTIVE),
            click_row("button.deleteAll", DESTRUCTIVE),
            click_row("button.deactivate-account", DESTRUCTIVE),
            click_row("a.cancel-order", DESTRUCTIVE),
            click_row("a.close-account", DESTRUCTIVE),
            click_row("#submitBtn", POSTING),
            click_row("button.publish", POSTING),
            click_row("button.post-comment", POSTING),
            click_row("button.send-message", POSTING),
            click_row("button#place-order", CHECKOUT),
            click_row("button.buy-now", CHECKOUT),
            click_row("button.pay-now", CHECKOUT),
        ]);
    }

    #[test]
    fn precedence_between_categories() {
        let checkout_page_login_button = row(
            "checkout beats login",
            action(
                ActionType::Click,
                "https://butikk.example.no/kasse",
                "button.logg-inn",
                "",
            ),
            None,
            CHECKOUT,
        );
        let cross_domain_first = row(
            "cross-domain beats checkout",
            action(
                ActionType::Goto,
                "https://betaling.example.no/kasse",
                "",
                "",
            ),
            Some(observation_at("https://butikk.example.no/handlekurv")),
            CROSS_DOMAIN,
        );
        // Posting needs an action that can actually submit.
        let posting_needs_a_click = row(
            "posting marker on a scroll",
            action(ActionType::Scroll, "", "button.send-inn", ""),
            None,
            NONE,
        );
        assert_table(&[
            checkout_page_login_button,
            click_row("a.slett-passord", LOGIN),
            click_row("button.delete-and-submit", DESTRUCTIVE),
            cross_domain_first,
            posting_needs_a_click,
        ]);
    }

    #[test]
    fn concrete_evidence_beats_the_planners_self_report() {
        let mut betal = button("Betal nå", CHECKOUT);
        betal.action.risk_category = Some(RiskCategory::Destructive);

        let mut password = typed_row(Some("Passord"), "x", LOGIN);
        password.action.risk_category = Some(RiskCategory::Checkout);

        // With nothing concrete to go on, the self-report still counts.
        let mut unlabelled = typed_row(None, "x", LOGIN);
        unlabelled.action.risk_category = Some(RiskCategory::Login);

        let mut harmless = link("Les mer", DESTRUCTIVE);
        harmless.action.risk_category = Some(RiskCategory::Destructive);

        assert_table(&[betal, password, unlabelled, harmless]);
    }

    #[test]
    fn a_ref_is_judged_by_its_own_target_not_its_neighbours() {
        let page = page_with(vec![
            snapshot_target("@e1", "button", Some("Slett konto"), Some("Slett konto")),
            snapshot_target("@e2", "link", Some("Hjem"), Some("Hjem")),
            snapshot_target("@e3", "textbox", Some("Passord"), None),
        ]);
        let click = |ref_id: &str| bound_to(action(ActionType::Click, "", "", ""), ref_id);
        assert_eq!(
            classify_action_risk(&click("@e1"), Some(&page)),
            DESTRUCTIVE
        );
        assert_eq!(classify_action_risk(&click("@e2"), Some(&page)), NONE);
        assert_eq!(classify_action_risk(&click("@e3"), Some(&page)), LOGIN);
    }

    #[test]
    fn a_ref_that_cannot_be_resolved_adds_no_evidence() {
        let page = page_with(vec![snapshot_target(
            "@e1",
            "button",
            Some("Slett konto"),
            None,
        )]);
        let unknown_ref = bound_to(action(ActionType::Click, "", "", ""), "@e9");
        assert_eq!(classify_action_risk(&unknown_ref, Some(&page)), NONE);

        let no_page = bound_to(action(ActionType::Click, "", "", ""), "@e1");
        assert_eq!(classify_action_risk(&no_page, None), NONE);
    }

    #[test]
    fn a_stale_ref_is_still_judged_by_the_target_it_names() {
        // Quarry rejects a ref from another generation, but the gate must not
        // lean on that: an action naming `@e1` is judged by `@e1`.
        let page = page_with(vec![snapshot_target(
            "@e1",
            "button",
            Some("Slett konto"),
            None,
        )]);
        let mut stale = bound_to(action(ActionType::Click, "", "", ""), "@e1");
        if let Some(target) = stale.target.as_mut() {
            target.generation = 7;
            target.snapshot_id = "snap_old".to_owned();
        }
        assert_eq!(classify_action_risk(&stale, Some(&page)), DESTRUCTIVE);
    }

    #[test]
    fn a_child_frame_target_is_resolved_too() {
        let mut field = snapshot_target("@e4", "textbox", Some("Passord"), None);
        field.frame_id = Some("frame_pay".to_owned());
        let page = page_with(vec![field]);
        let mut typed = bound_to(action(ActionType::Type, "", "", "x"), "@e4");
        if let Some(target) = typed.target.as_mut() {
            target.frame_id = Some("frame_pay".to_owned());
        }
        assert_eq!(classify_action_risk(&typed, Some(&page)), LOGIN);
    }

    #[test]
    fn only_click_and_type_read_their_target() {
        let page = page_with(vec![snapshot_target(
            "@e1",
            "button",
            Some("Slett konto"),
            Some("Slett konto"),
        )]);
        for action_type in [
            ActionType::Scroll,
            ActionType::Wait,
            ActionType::Extract,
            ActionType::Observe,
        ] {
            let a = bound_to(action(action_type.clone(), "", "", ""), "@e1");
            assert_eq!(
                classify_action_risk(&a, Some(&page)),
                NONE,
                "{} must not be judged by a target it does not operate",
                action_type.as_str()
            );
        }
    }

    #[test]
    fn a_click_reads_name_and_text_but_a_type_reads_only_role_and_name() {
        // A text field's `text` is its current content (Quarry falls back to
        // the value when there is no name), which is user data.
        let content_only = page_with(vec![snapshot_target(
            "@e1",
            "textbox",
            Some("Søk"),
            Some("slett konto"),
        )]);
        let typed = bound_to(action(ActionType::Type, "", "", "x"), "@e1");
        assert_eq!(classify_action_risk(&typed, Some(&content_only)), NONE);

        let text_only = page_with(vec![snapshot_target(
            "@e1",
            "link",
            None,
            Some("Slett konto"),
        )]);
        let click = bound_to(action(ActionType::Click, "", "", ""), "@e1");
        assert_eq!(classify_action_risk(&click, Some(&text_only)), DESTRUCTIVE);
    }

    #[test]
    fn words_split_at_punctuation_humps_and_digits() {
        let cases: &[(&str, &[&str])] = &[
            ("btnDeleteAccount", &["btn", "delete", "account"]),
            ("HTMLParser", &["html", "parser"]),
            ("kasse-og-betaling", &["kasse", "og", "betaling"]),
            ("a.slett_konto", &["a", "slett", "konto"]),
            ("step2Pay", &["step", "2", "pay"]),
            ("Fullfør bestilling", &["fullfør", "bestilling"]),
            ("KJØP NÅ", &["kjøp", "nå"]),
            ("input[type=password]", &["input", "type", "password"]),
            (
                "https://kasse.example.no/",
                &["https", "kasse", "example", "no"],
            ),
            ("  ", &[]),
            ("", &[]),
        ];
        for (text, expected) in cases {
            assert_eq!(&split_words(text), expected, "split_words({text:?})");
        }
    }

    #[test]
    fn percent_escapes_are_decoded_and_stray_percents_are_left_alone() {
        let cases = [
            ("/log%69n", "/login"),
            ("/logg%20inn", "/logg inn"),
            ("/kj%C3%B8p-n%C3%A5", "/kjøp-nå"),
            ("/LOG%49N", "/LOGIN"),
            ("100%", "100%"),
            ("50%+off", "50%+off"),
            ("/%zz", "/%zz"),
            ("/%4", "/%4"),
            ("/%+1", "/%+1"),
            ("plain", "plain"),
            ("slett-konto", "slett-konto"),
        ];
        for (raw, decoded) in cases {
            assert_eq!(percent_decode(raw), decoded, "percent_decode({raw:?})");
        }
    }

    #[test]
    fn marker_tables_are_written_in_the_form_the_matcher_compares() {
        // A marker the matcher can never see (an upper-case letter, a stray
        // hyphen in a `Word`, a doubled space) would be dead weight that
        // looks like coverage.
        let tables = [
            ("checkout", CHECKOUT_MARKERS),
            ("login", LOGIN_MARKERS),
            ("destructive", DESTRUCTIVE_MARKERS),
            ("posting", POSTING_MARKERS),
        ];
        for (table, markers) in tables {
            for &marker in markers {
                match marker {
                    Marker::Anywhere(text) => {
                        assert!(!text.is_empty(), "{table}: empty fragment");
                        assert_eq!(
                            text,
                            text.to_lowercase(),
                            "{table}: {text:?} not lower-case"
                        );
                    }
                    Marker::Verb(text) => assert!(
                        !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase()),
                        "{table}: verb {text:?} must be plain lower-case letters"
                    ),
                    Marker::Word(text) => assert_eq!(
                        split_words(text).join(" "),
                        text,
                        "{table}: word {text:?} is not in the split, lower-case, single-spaced form"
                    ),
                }
            }
        }
    }
}
