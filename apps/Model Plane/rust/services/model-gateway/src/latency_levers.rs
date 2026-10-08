//! Kill-switches for the latency levers that change what the model is sent on
//! the recorded journeys.
//!
//! Each lever is on unless its variable is set to `0`, `off`, `false`, `no` or
//! `disabled`. Off restores exactly the request the gateway sent before that
//! lever, so a regression found in live validation is rolled back by
//! configuration and a restart, without a rebuild.
//!
//! The one exception is an OPT-IN lever ([`LatencyLevers::dated_post_section_only`]):
//! it is off unless its variable is set to `1`, `on`, `true`, `yes` or
//! `enabled`, so an unset, empty or unrecognised value keeps it off. It is not
//! part of [`LatencyLevers::ALL_ON`], which stays the set of levers that are
//! on by default in code. The shipped compose file sets it to `1` (Q16h,
//! 2026-09-29), so a deployment runs with it unless
//! `MODEL_GATEWAY_DATED_POST_SECTION_ONLY=0` is set. The same holds for the
//! second opt-in lever, [`LatencyLevers::finer_review_batches`], which the
//! compose file sets to `1` since the A/B of 2026-09-30
//! (`MODEL_GATEWAY_FINER_REVIEW_BATCHES=0` turns it off). The code reads an
//! empty value as off, but the compose file's `:-1` fills an empty value in
//! with `1`, so in a deployment only an explicit `0` turns either one off.
//!
//! The values are read once per process ([`LatencyLevers::from_env`]) and
//! carried on `AppState`, so a turn never changes its mind part-way. Tests set
//! them on the state or pass them to the function under test; they never
//! mutate the environment.

/// Switch for [`LatencyLevers::sole_note_prefetch`] — the sole-artifact
/// prefetch for a new note's source or a scoped revision's document.
pub const SOLE_NOTE_PREFETCH_ENV: &str = "MODEL_GATEWAY_SOLE_NOTE_PREFETCH";

/// Switch for [`LatencyLevers::recheck_proposes_edits`].
pub const RECHECK_PROPOSES_EDITS_ENV: &str = "MODEL_GATEWAY_RECHECK_PROPOSES_EDITS";

/// Switch for [`LatencyLevers::balanced_review_batches`].
pub const BALANCED_REVIEW_BATCHES_ENV: &str = "MODEL_GATEWAY_BALANCED_REVIEW_BATCHES";

/// Switch for [`LatencyLevers::revision_review_reuse`].
pub const REVISION_REVIEW_REUSE_ENV: &str = "MODEL_GATEWAY_REVISION_REVIEW_REUSE";

/// Switch for [`LatencyLevers::revision_framing_guard`].
pub const REVISION_FRAMING_GUARD_ENV: &str = "MODEL_GATEWAY_REVISION_FRAMING_GUARD";

/// Switch for [`LatencyLevers::in_turn_scoped_recheck`].
pub const INTURN_SCOPED_RECHECK_ENV: &str = "MODEL_GATEWAY_INTURN_SCOPED_RECHECK";

/// Switch for [`LatencyLevers::tiny_candidate_skips_propose_edits`].
pub const TINY_CANDIDATE_SKIPS_PROPOSE_EDITS_ENV: &str =
    "MODEL_GATEWAY_TINY_CANDIDATE_SKIPS_PROPOSE_EDITS";

/// Switch for [`LatencyLevers::targeted_source_repair`].
pub const TARGETED_SOURCE_REPAIR_ENV: &str = "MODEL_GATEWAY_TARGETED_SOURCE_REPAIR";

/// Switch for [`LatencyLevers::list_item_segments`].
pub const LIST_ITEM_SEGMENTS_ENV: &str = "MODEL_GATEWAY_LIST_ITEM_SEGMENTS";

/// Switch for [`LatencyLevers::dated_post_section_only`]. Opt-in: only an
/// explicit on word turns it on.
pub const DATED_POST_SECTION_ONLY_ENV: &str = "MODEL_GATEWAY_DATED_POST_SECTION_ONLY";

/// Switch for [`LatencyLevers::finer_review_batches`]. Opt-in: only an
/// explicit on word turns it on.
pub const FINER_REVIEW_BATCHES_ENV: &str = "MODEL_GATEWAY_FINER_REVIEW_BATCHES";

/// Which latency levers are in force. The default is every lever that is on by
/// default on ([`Self::ALL_ON`]); an opt-in lever stays off until enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyLevers {
    /// A new note whose thread holds one completely readable prior artifact,
    /// OR a scoped revision of that thread's one completely readable
    /// artifact, has that artifact read by the gateway before the first
    /// model round (`tool_loop::sole_artifact_prefetch`). Off: the model
    /// decides whether to read it, as before.
    pub sole_note_prefetch: bool,
    /// Every source-review pass that a repair can still follow may propose
    /// edits, the recheck included (`source_validation`). Off: only the first
    /// pass may, as before.
    pub recheck_proposes_edits: bool,
    /// A split source review is batched by the lightest contiguous split
    /// (`source_validation`). Off: by the heaviest-range bisection, as before.
    pub balanced_review_batches: bool,
    /// A revision turn on an already-accepted, source-reviewed artifact
    /// version may reuse that version's segment approvals for byte-identical,
    /// unaffected segments (`source_validation::check_artifact`), sending only
    /// the changed and dependent segments to the reviewer. Off: every segment
    /// is reviewed fresh, exactly as before this lever existed (owner ruling,
    /// 2026-09-26: "receipt-bound reuse on revision turns").
    pub revision_review_reuse: bool,
    /// On a scoped revision, a reviewer edit inside the targeted post that
    /// deletes a requested framing/audience content word (letters only, at
    /// least 4 letters, case-insensitive) named in the revision instruction
    /// is refused; that pass's reviewer edits are then all unavailable and
    /// the check falls back to a model repair naming the kept terms
    /// (`source_validation::check_artifact`, owner ruling 2026-09-27,
    /// "framing guard"; Q16b take 13 lost "flere team" this way). Off:
    /// today's edit handling, byte for byte.
    pub revision_framing_guard: bool,
    /// Owner ruling 2026-09-27, "in-turn recheck scope on every turn": after
    /// an edit or repair within a turn, on ANY turn (a first-time draft or a
    /// new note included, not only a scoped revision), the recheck assigns
    /// only the segments changed since the immediately previous reviewed
    /// candidate and their dependents (same section, or a shared CHANGED
    /// date, amount or proper name), under the same dependency rule as a
    /// scoped revision's own carryover; a segment that pass approved and
    /// that stays byte-identical, unchanged section included, is carried
    /// with its evidence and never resent (`source_validation::check_artifact`).
    /// When a scoped revision's own cross-turn carryover
    /// (`revision_review_reuse`) also applies, this narrows that carryover's
    /// own assignment further by the same rule, on top of it. Measured on
    /// the third freeze: complete rechecks caused 4 of 8 failed attempts as
    /// rejection cascades, each rejecting a DIFFERENT segment, against 14%
    /// rejecting passes and no cascades for scoped-revision rechecks; a
    /// rejecting single reused review call took 39s against 22s for a
    /// rejecting batch. Off: today's independent complete recheck on every
    /// in-turn repair, byte for byte.
    pub in_turn_scoped_recheck: bool,
    /// A first-pass source review of a candidate with fewer than
    /// `REUSED_REVIEW_BATCH_MIN` segments is not offered the private-edit
    /// tool (`source_validation::review_candidate`'s `propose_edits` gate):
    /// too few segments means there is nothing left to narrow a forced full
    /// recheck by, so a rejection goes straight to the model repair instead
    /// of paying for a free edit attempt and, when it fails to fix the
    /// claim, an extra full review pass before repair even starts. The
    /// invariant that a reviewer suggestion is never a verdict is untouched
    /// for every larger document, which still gets the free-edit attempt.
    /// Investigation: freeze-5 journey 04-prosjektplan attempt 4 spent
    /// ~26.5s of its 74.6s tail on exactly this pattern, on a 2-segment
    /// candidate. Off: every pass that can still be repaired may propose
    /// edits regardless of segment count, as before this lever existed.
    pub tiny_candidate_skips_propose_edits: bool,
    /// A source-review repair populates its repair ranges from the claim
    /// failures' segment indexes, the same targeted treatment a
    /// `campaign_contract` document-check repair already gets
    /// (`source_validation::check_artifact`), so the repair request and its
    /// `"segments"` payload carry only the failed segment(s) plus enough
    /// surrounding context for the prompt, not the whole document. Off:
    /// `section_ranges` stays empty for a source failure and the repair
    /// resends every segment, as before this lever existed.
    pub targeted_source_repair: bool,
    /// A Markdown LIST block splits into one review segment per TOP-LEVEL
    /// item — ordered or unordered, task items included — exactly parallel
    /// to how a table already splits into one segment per row
    /// (`source_validation::segments`/`split_list_block`). A nested
    /// sub-item, a continuation line and a fenced code block inside an item
    /// stay part of that item's own segment; a heading or paragraph
    /// immediately before a list with no blank line between them (so both
    /// share one `paragraphs` block) still gets its own leading segment.
    /// Investigation: freeze-5 journeys 03-kampanje and 04-prosjektplan's
    /// three slow repair-tail turns each rejected one kind-`list` segment,
    /// forcing the reviewer to examine every bullet in one call (1799-2078
    /// output tokens against 260-614 for a clean segment) and the repair/
    /// recheck to re-cover the whole list; batch latency followed the
    /// segment's own content, not its neighbor count. Off: a list stays one
    /// segment, as before this lever existed.
    pub list_item_segments: bool,
    /// OPT-IN, off by default (`MODEL_GATEWAY_DATED_POST_SECTION_ONLY`, empty
    /// or unset means off). Stage 1 of the patch-revision plan
    /// (`plans/system-audits/patch-revision-2026-09-29`, critic verdict
    /// section 6): on a turn whose instruction
    /// [`crate::revision_preservation::Preservation::from_prompt`] recognises
    /// as a revision of one dated post, the model is told to send
    /// `update_artifact` with ONLY that post's section (its heading line and
    /// body), which the gateway splices into the accepted document with the
    /// same `Preservation::apply` a full-document candidate already gets. A
    /// section whose heading line is missing gets the original heading
    /// prepended first; a full document still works exactly as before. The
    /// spliced document, never the fragment, is what is checked, stored,
    /// emitted and persisted. Scope stays exactly what
    /// `Preservation::targeted_piece` names. Off: the tool description, the
    /// per-turn guidance and the candidate handling are those of every other
    /// turn, byte for byte.
    pub dated_post_section_only: bool,
    /// OPT-IN, off by default (`MODEL_GATEWAY_FINER_REVIEW_BATCHES`, empty or
    /// unset means off). A source review that is split into batches (a
    /// subscription review of twelve segments or more, or a scoped recheck
    /// of three or more) uses one batch per four segments or per 1,400 bytes
    /// of weight (the segment's bytes plus 128), whichever is more, instead of
    /// one per six or per 2,200, never fewer than two nor more than four
    /// (`source_validation::review_groups`). Where the old rule already gave
    /// four batches it changes nothing. Reason: in Q16k
    /// the slow review batches were the big ones. Of 137 batches with seven
    /// segments or more, 8 took over 30 s, against 3 of 121 smaller ones, and
    /// two journey-04 first turns timed out on a single 7-segment, 1,400-byte
    /// table batch that spent 4,000 to 4,600 reasoning tokens (53 to 61 s)
    /// while its two neighbours finished in 11 to 13 s. The number of
    /// concurrent batches is still capped at four. Off: the batch count and
    /// the split are those of every other review, byte for byte.
    pub finer_review_batches: bool,
}

impl LatencyLevers {
    /// Every lever that is on by default on: the default. The opt-in
    /// [`Self::dated_post_section_only`] and [`Self::finer_review_batches`] are NOT part of it — a test that wants
    /// section-only output says so (`LatencyLevers { dated_post_section_only:
    /// true, ..LatencyLevers::ALL_ON }`), so every existing use of this set
    /// keeps sending the requests it always did.
    pub const ALL_ON: Self = Self {
        sole_note_prefetch: true,
        recheck_proposes_edits: true,
        balanced_review_batches: true,
        revision_review_reuse: true,
        revision_framing_guard: true,
        in_turn_scoped_recheck: true,
        tiny_candidate_skips_propose_edits: true,
        targeted_source_repair: true,
        list_item_segments: true,
        dated_post_section_only: false,
        finer_review_batches: false,
    };

    /// Every lever off: the requests the gateway sent before any of them.
    pub const ALL_OFF: Self = Self {
        sole_note_prefetch: false,
        recheck_proposes_edits: false,
        balanced_review_batches: false,
        revision_review_reuse: false,
        revision_framing_guard: false,
        in_turn_scoped_recheck: false,
        tiny_candidate_skips_propose_edits: false,
        targeted_source_repair: false,
        list_item_segments: false,
        dated_post_section_only: false,
        finer_review_batches: false,
    };

    /// The operator's settings, from the eleven flag values.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn parse(
        sole_note_prefetch: Option<&str>,
        recheck_proposes_edits: Option<&str>,
        balanced_review_batches: Option<&str>,
        revision_review_reuse: Option<&str>,
        revision_framing_guard: Option<&str>,
        in_turn_scoped_recheck: Option<&str>,
        tiny_candidate_skips_propose_edits: Option<&str>,
        targeted_source_repair: Option<&str>,
        list_item_segments: Option<&str>,
        dated_post_section_only: Option<&str>,
        finer_review_batches: Option<&str>,
    ) -> Self {
        Self {
            sole_note_prefetch: switched_on(sole_note_prefetch),
            recheck_proposes_edits: switched_on(recheck_proposes_edits),
            balanced_review_batches: switched_on(balanced_review_batches),
            revision_review_reuse: switched_on(revision_review_reuse),
            revision_framing_guard: switched_on(revision_framing_guard),
            in_turn_scoped_recheck: switched_on(in_turn_scoped_recheck),
            tiny_candidate_skips_propose_edits: switched_on(tiny_candidate_skips_propose_edits),
            targeted_source_repair: switched_on(targeted_source_repair),
            list_item_segments: switched_on(list_item_segments),
            dated_post_section_only: switched_on_explicitly(dated_post_section_only),
            finer_review_batches: switched_on_explicitly(finer_review_batches),
        }
    }

    /// The operator's settings, read once like
    /// `prompt_context::CacheOptions::from_env`.
    #[must_use]
    pub fn from_env() -> Self {
        static LEVERS: std::sync::OnceLock<LatencyLevers> = std::sync::OnceLock::new();
        *LEVERS.get_or_init(|| {
            let read = |name: &str| std::env::var(name).ok();
            let levers = Self::parse(
                read(SOLE_NOTE_PREFETCH_ENV).as_deref(),
                read(RECHECK_PROPOSES_EDITS_ENV).as_deref(),
                read(BALANCED_REVIEW_BATCHES_ENV).as_deref(),
                read(REVISION_REVIEW_REUSE_ENV).as_deref(),
                read(REVISION_FRAMING_GUARD_ENV).as_deref(),
                read(INTURN_SCOPED_RECHECK_ENV).as_deref(),
                read(TINY_CANDIDATE_SKIPS_PROPOSE_EDITS_ENV).as_deref(),
                read(TARGETED_SOURCE_REPAIR_ENV).as_deref(),
                read(LIST_ITEM_SEGMENTS_ENV).as_deref(),
                read(DATED_POST_SECTION_ONLY_ENV).as_deref(),
                read(FINER_REVIEW_BATCHES_ENV).as_deref(),
            );
            tracing::info!(
                sole_note_prefetch = levers.sole_note_prefetch,
                recheck_proposes_edits = levers.recheck_proposes_edits,
                balanced_review_batches = levers.balanced_review_batches,
                revision_review_reuse = levers.revision_review_reuse,
                revision_framing_guard = levers.revision_framing_guard,
                in_turn_scoped_recheck = levers.in_turn_scoped_recheck,
                tiny_candidate_skips_propose_edits = levers.tiny_candidate_skips_propose_edits,
                targeted_source_repair = levers.targeted_source_repair,
                list_item_segments = levers.list_item_segments,
                dated_post_section_only = levers.dated_post_section_only,
                finer_review_batches = levers.finer_review_batches,
                "latency levers"
            );
            levers
        })
    }
}

impl Default for LatencyLevers {
    fn default() -> Self {
        Self::ALL_ON
    }
}

/// On unless the value is one of the off words. Unset, empty or unrecognised
/// keeps the lever on.
fn switched_on(value: Option<&str>) -> bool {
    !matches!(
        value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no" | "disabled"
    )
}

/// The opt-in counterpart of [`switched_on`]: on only for an explicit on
/// word. Unset, empty, an off word or anything unrecognised keeps it off, so
/// an empty passthrough from the deployment file is off, not on.
fn switched_on_explicitly(value: Option<&str>) -> bool {
    matches!(
        value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "on" | "true" | "yes" | "enabled"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lever_is_on_unless_switched_off() {
        assert_eq!(LatencyLevers::default(), LatencyLevers::ALL_ON);
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, None, None, None, None, None, None, None),
            LatencyLevers::ALL_ON
        );
        // The nine default-on levers; the opt-in tenth is left unset here and
        // has its own test below.
        for on in ["", "1", "on", "true", "yes", "enabled", "sometimes"] {
            assert_eq!(
                LatencyLevers::parse(Some(on), Some(on), Some(on), Some(on), Some(on), Some(on), Some(on), Some(on), Some(on), None, None),
                LatencyLevers::ALL_ON,
                "{on:?}"
            );
        }
        for off in [
            "0", "off", "false", "no", "disabled", " OFF ", "False", "No\n",
        ] {
            assert_eq!(
                LatencyLevers::parse(Some(off), Some(off), Some(off), Some(off), Some(off), Some(off), Some(off), Some(off), Some(off), Some(off), None),
                LatencyLevers::ALL_OFF,
                "{off:?}"
            );
        }
    }

    /// The opt-in lever: off unless explicitly enabled, so the empty
    /// passthrough a deployment file writes, an unset variable and any
    /// unrecognised word all leave it off, and it never disturbs the nine
    /// default-on levers in either direction.
    #[test]
    fn the_section_only_lever_is_opt_in() {
        assert!(!LatencyLevers::default().dated_post_section_only);
        assert!(!LatencyLevers::ALL_ON.dated_post_section_only, "ALL_ON is the default set, not everything");
        assert!(!LatencyLevers::ALL_OFF.dated_post_section_only);
        for off in [None, Some(""), Some("  "), Some("0"), Some("off"), Some("false"), Some("no"), Some("disabled"), Some("sometimes"), Some("2")] {
            assert_eq!(
                LatencyLevers::parse(None, None, None, None, None, None, None, None, None, off, None),
                LatencyLevers::ALL_ON,
                "{off:?}"
            );
        }
        for on in ["1", "on", "true", "yes", "enabled", " ON ", "True", "YES\n"] {
            assert_eq!(
                LatencyLevers::parse(None, None, None, None, None, None, None, None, None, Some(on), None),
                LatencyLevers { dated_post_section_only: true, ..LatencyLevers::ALL_ON },
                "{on:?}"
            );
            assert_eq!(
                LatencyLevers::parse(Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some(on), None),
                LatencyLevers { dated_post_section_only: true, ..LatencyLevers::ALL_OFF },
                "{on:?}: it does not depend on the other nine"
            );
        }
    }

    /// The second opt-in lever, finer review batches: off unless explicitly
    /// enabled, independent of the section-only lever and of the nine that are
    /// on by default.
    #[test]
    fn the_finer_batches_lever_is_opt_in_and_independent() {
        assert!(!LatencyLevers::default().finer_review_batches);
        assert!(!LatencyLevers::ALL_ON.finer_review_batches, "ALL_ON is the default set, not everything");
        assert!(!LatencyLevers::ALL_OFF.finer_review_batches);
        for off in [None, Some(""), Some("  "), Some("0"), Some("off"), Some("false"), Some("no"), Some("disabled"), Some("sometimes"), Some("2")] {
            assert_eq!(
                LatencyLevers::parse(None, None, None, None, None, None, None, None, None, None, off),
                LatencyLevers::ALL_ON,
                "{off:?}"
            );
        }
        for on in ["1", "on", "true", "yes", "enabled", " ON ", "True", "YES\n"] {
            assert_eq!(
                LatencyLevers::parse(None, None, None, None, None, None, None, None, None, None, Some(on)),
                LatencyLevers { finer_review_batches: true, ..LatencyLevers::ALL_ON },
                "{on:?}"
            );
            assert_eq!(
                LatencyLevers::parse(Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some("0"), Some(on)),
                LatencyLevers { finer_review_batches: true, ..LatencyLevers::ALL_OFF },
                "{on:?}: it does not depend on the others"
            );
            assert_eq!(
                LatencyLevers::parse(None, None, None, None, None, None, None, None, None, Some(on), Some(on)),
                LatencyLevers { dated_post_section_only: true, finer_review_batches: true, ..LatencyLevers::ALL_ON },
                "{on:?}: nor does the section-only lever depend on it"
            );
        }
    }

    #[test]
    fn each_switch_turns_off_only_its_own_lever() {
        assert_eq!(
            LatencyLevers::parse(Some("0"), None, None, None, None, None, None, None, None, None, None),
            LatencyLevers {
                sole_note_prefetch: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, Some("off"), None, None, None, None, None, None, None, None, None),
            LatencyLevers {
                recheck_proposes_edits: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, Some("disabled"), None, None, None, None, None, None, None, None),
            LatencyLevers {
                balanced_review_batches: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, Some("0"), None, None, None, None, None, None, None),
            LatencyLevers {
                revision_review_reuse: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, Some("0"), None, None, None, None, None, None),
            LatencyLevers {
                revision_framing_guard: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, None, Some("0"), None, None, None, None, None),
            LatencyLevers {
                in_turn_scoped_recheck: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, None, None, Some("0"), None, None, None, None),
            LatencyLevers {
                tiny_candidate_skips_propose_edits: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, None, None, None, Some("0"), None, None, None),
            LatencyLevers {
                targeted_source_repair: false,
                ..LatencyLevers::ALL_ON
            }
        );
        assert_eq!(
            LatencyLevers::parse(None, None, None, None, None, None, None, None, Some("0"), None, None),
            LatencyLevers {
                list_item_segments: false,
                ..LatencyLevers::ALL_ON
            }
        );
    }

    /// The process value is read once. Without the variables set it is every
    /// default-on lever on and the opt-in one off; with them set it is what
    /// they say.
    #[test]
    fn the_process_setting_is_read_from_the_eleven_variables() {
        let read = |name: &str| std::env::var(name).ok();
        let expected = LatencyLevers::parse(
            read(SOLE_NOTE_PREFETCH_ENV).as_deref(),
            read(RECHECK_PROPOSES_EDITS_ENV).as_deref(),
            read(BALANCED_REVIEW_BATCHES_ENV).as_deref(),
            read(REVISION_REVIEW_REUSE_ENV).as_deref(),
            read(REVISION_FRAMING_GUARD_ENV).as_deref(),
            read(INTURN_SCOPED_RECHECK_ENV).as_deref(),
            read(TINY_CANDIDATE_SKIPS_PROPOSE_EDITS_ENV).as_deref(),
            read(TARGETED_SOURCE_REPAIR_ENV).as_deref(),
            read(LIST_ITEM_SEGMENTS_ENV).as_deref(),
            read(DATED_POST_SECTION_ONLY_ENV).as_deref(),
            read(FINER_REVIEW_BATCHES_ENV).as_deref(),
        );
        assert_eq!(LatencyLevers::from_env(), expected);
        assert_eq!(LatencyLevers::from_env(), LatencyLevers::from_env());
    }
}
