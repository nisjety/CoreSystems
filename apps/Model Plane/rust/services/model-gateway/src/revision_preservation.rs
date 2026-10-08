//! Copy protected Markdown sections from the accepted artifact, byte for byte.
//! This intentionally supports a small set of unambiguous revision commands;
//! it is not a claim to understand every natural-language preservation request.

use std::borrow::Cow;
use std::ops::Range;

#[derive(Clone, Debug)]
struct Section {
    label: String,
    /// Heading level, 1-6. A standalone bold label counts as level 6.
    level: usize,
    range: Range<usize>,
}

fn sections(text: &str) -> Vec<Section> {
    let mut headings = Vec::new();
    let mut offset = 0;
    let mut fence = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let trimmed = trimmed.trim_start_matches('>').trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
        }
        if !fence {
            let level = trimmed.chars().take_while(|ch| *ch == '#').count();
            if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
                headings.push((offset, level, trimmed[level..].trim().to_lowercase()));
            } else if let Some(label) = trimmed
                .strip_prefix("**")
                .and_then(|text| text.strip_suffix("**"))
            {
                // A standalone bold label is also how the authored customer
                // document marks its quoted internal note. Inline bold in a
                // paragraph or list is not a section boundary.
                if !label.contains("**") && !label.trim().is_empty() {
                    headings.push((offset, 6, label.trim().to_lowercase()));
                }
            }
        }
        offset += line.len();
    }
    headings
        .iter()
        .enumerate()
        .map(|(index, (start, level, label))| Section {
            label: label.clone(),
            level: *level,
            range: *start
                ..headings[index + 1..]
                    .iter()
                    .find(|(_, next, _)| next <= level)
                    .map_or(text.len(), |(start, _, _)| *start),
        })
        .collect()
}

#[derive(Clone, Debug)]
pub enum Preservation {
    InternalNotes {
        before: String,
    },
    DatedPost {
        before: String,
        day: String,
        month: String,
    },
}

fn internal(label: &str) -> bool {
    label.contains("intern") || label.contains("kildeoversikt") || label.contains("source notes")
}

/// How many sections of `text` carry an internal-note heading, and how many
/// headings it has in all. For a content-free diagnostic only: a customer draft
/// needs exactly ONE internal-note heading, so zero (none written, or named
/// something else) and two or more (a note plus a sub-note) are the two ways a
/// draft is reported as missing its sections. Q16k journey 01, take 16.
pub fn internal_heading_counts(text: &str) -> (usize, usize) {
    let all = sections(text);
    (all.iter().filter(|section| internal(&section.label)).count(), all.len())
}

pub fn internal_notes_start(text: &str) -> Option<usize> {
    internal_section(text).ok().map(|range| range.start)
}

fn internal_section(text: &str) -> Result<Range<usize>, String> {
    let mut range = unique_section(text, internal)?;
    if text[range.start..].trim_start().starts_with('>') {
        // A quoted note is one container, including bold subheadings such as
        // "Sources" and "Status". Treating those as peer sections preserved
        // only the label while allowing the actual source table to change.
        let mut end = range.start;
        for line in text[range.start..].split_inclusive('\n') {
            if !line.trim().is_empty() && !line.trim_start().starts_with('>') {
                break;
            }
            end += line.len();
        }
        range.end = end;
    }
    Ok(range)
}

fn unique_section(text: &str, matches: impl Fn(&str) -> bool) -> Result<Range<usize>, String> {
    unique_heading_section(text, matches).map(|section| section.range)
}

/// [`unique_section`] with the section's heading level and label as well.
fn unique_heading_section(text: &str, matches: impl Fn(&str) -> bool) -> Result<Section, String> {
    let mut found: Vec<_> = sections(text)
        .into_iter()
        .filter(|section| matches(&section.label))
        .collect();
    if found.len() != 1 {
        return Err("The requested section must have one distinct Markdown heading; do not remove, duplicate or rename it.".into());
    }
    Ok(found.remove(0))
}

fn dated(label: &str, day: &str, month: &str) -> bool {
    (label.contains("innlegg")
        || label.contains("linkedin")
        || label.contains("post")
        || label.starts_with(&format!("{day}.")))
        && label
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|word| word == day)
        && label.contains(month)
}

impl Preservation {
    pub fn from_prompt(prompt: &str, before: &str) -> Result<Option<Self>, String> {
        let instruction = prompt
            .split("\n\n--- VEDLEGG:")
            .next()
            .unwrap_or(prompt)
            .trim()
            .to_lowercase();
        if (instruction.contains("behold")
            || instruction.contains("preserve")
            || instruction.contains("keep"))
            && [
                "interne kildeoversikten",
                "intern merknad",
                "internal notes",
                "source notes",
            ]
            .iter()
            .any(|term| instruction.contains(term))
        {
            internal_section(before)?;
            return Ok(Some(Self::InternalNotes {
                before: before.to_owned(),
            }));
        }
        let target = [
            "gjør innlegget for ",
            "endre innlegget for ",
            "revise the post for ",
            "make the post for ",
        ]
        .iter()
        .find_map(|prefix| instruction.strip_prefix(prefix));
        if let Some((day, rest)) = target.and_then(|target| target.split_once('.')) {
            // A named-post prefix is not authority to discard additional edits
            // explicitly requested in the same message. Those mixed requests
            // stay on the ordinary whole-document revision path.
            if [
                "og oppdater",
                "og endre",
                "og revider",
                "og innlegget",
                "og for ",
                // Taking the note (or anything else) away is a second edit: it
                // stays on the whole-document path, where a section without its
                // sub-section is not refused.
                "og fjern",
                "og slett",
                "og ta bort",
                "uten kilde",
                "and update",
                "and change",
                "and revise",
                "and the post",
                "and remove",
                "and delete",
                "without the source",
                "also update",
            ]
            .iter()
            .any(|clause| rest.contains(clause))
            {
                return Ok(None);
            }
            let month: String = rest
                .trim_start()
                .chars()
                .take_while(|ch| ch.is_alphabetic())
                .collect();
            let day = day.trim().to_owned();
            if !day.parse::<u8>().is_ok_and(|day| (1..=31).contains(&day)) || month.is_empty() {
                return Ok(None);
            }
            unique_section(before, |label| dated(label, &day, &month))?;
            return Ok(Some(Self::DatedPost {
                before: before.to_owned(),
                day,
                month,
            }));
        }
        Ok(None)
    }

    /// The instruction's specific dated-post target, when it names exactly
    /// one via the recognized "make/revise the post for D. Month" phrasing,
    /// as a byte range into `before` — for `source_validation`'s revision-turn
    /// reuse, which must not treat any OTHER instruction as naming a single
    /// piece (a whole-document or unrecognised request stays whole-document,
    /// "when in doubt, review it"). `Ok(None)` means no such piece is named.
    /// `Err` only when the named piece cannot be uniquely resolved, exactly
    /// as [`Self::from_prompt`] would error on it.
    pub(crate) fn targeted_piece(prompt: &str, before: &str) -> Result<Option<Range<usize>>, String> {
        Ok(match Self::from_prompt(prompt, before)? {
            Some(Self::DatedPost { day, month, .. }) => {
                Some(unique_section(before, |label| dated(label, &day, &month))?)
            }
            _ => None,
        })
    }

    pub fn apply(&self, candidate: &str) -> Result<String, String> {
        match self {
            Self::InternalNotes { before } => {
                let old = internal_section(before)?;
                let new = internal_section(candidate)?;
                Ok(format!(
                    "{}{}{}",
                    &candidate[..new.start],
                    &before[old],
                    &candidate[new.end..]
                ))
            }
            Self::DatedPost { before, day, month } => {
                let old = unique_section(before, |label| dated(label, day, month))?;
                let new = unique_section(candidate, |label| dated(label, day, month))?;
                // Copy the original boundary whitespace too: a model removing
                // the blank line must not merge the following heading into prose.
                let replacement = candidate[new].trim_end();
                let old_text = &before[old.clone()];
                let trailing = &old_text[old_text.trim_end().len()..];
                Ok(format!(
                    "{}{replacement}{trailing}{}",
                    &before[..old.start],
                    &before[old.end..]
                ))
            }
        }
    }

    /// Section-only output (`LatencyLevers::dated_post_section_only`): the
    /// candidate a dated-post revision's model sent, ready for [`Self::apply`].
    ///
    /// `apply` already takes the target section out of whatever it is given
    /// and splices it into the accepted document, so a fragment that holds the
    /// section (its heading line and body, alone or beside other sections)
    /// needs nothing: it is returned as it is, and a fragment with several
    /// sections is spliced by the one whose label matches, exactly as a whole
    /// document is. What `apply` cannot do is find a section whose heading
    /// line the model left off. Only then is the ORIGINAL section's heading
    /// line put in front of the candidate, and only when the candidate has no
    /// heading at the target's level or above at all and none that matches
    /// the target. A candidate that carries some other heading at that level
    /// — a wrong date, a whole document that dropped the post, a title — is
    /// returned untouched, so `apply` refuses it as it does today.
    ///
    /// Every other preservation, and a target the accepted document does not
    /// resolve uniquely, returns the candidate untouched.
    pub(crate) fn with_section_heading<'a>(&self, candidate: &'a str) -> Cow<'a, str> {
        let Self::DatedPost { before, day, month } = self else {
            return Cow::Borrowed(candidate);
        };
        let Ok(target) = unique_heading_section(before, |label| dated(label, day, month)) else {
            return Cow::Borrowed(candidate);
        };
        if sections(candidate)
            .iter()
            .any(|section| section.level <= target.level || dated(&section.label, day, month))
        {
            return Cow::Borrowed(candidate);
        }
        let heading = before[target.range].split_inclusive('\n').next().unwrap_or_default();
        let mut completed = String::with_capacity(heading.len() + 1 + candidate.len());
        completed.push_str(heading);
        if !heading.ends_with('\n') {
            completed.push('\n');
        }
        completed.push_str(candidate);
        Cow::Owned(completed)
    }

    /// Whether `candidate` is a section-only output rather than a whole
    /// document, for the turn's timing record: whether the splice took the
    /// model's section out of a fragment. What the candidate holds OUTSIDE the
    /// target section is under half of what the accepted document holds
    /// outside it (whitespace at either edge does not count): a model that
    /// sent the document back, however much it changed, sends about as much
    /// outside the section as there was; one that sent the section sends none,
    /// or a neighbour or two. `complete_heading` says whether the check
    /// completes a missing heading line first
    /// ([`Self::with_section_heading`], the `dated_post_section_only` lever):
    /// without it a section whose heading was left off cannot be spliced at
    /// all, so it is not counted as spliced. `false` for any other
    /// preservation and for a target either side cannot resolve.
    pub(crate) fn is_section_fragment(&self, candidate: &str, complete_heading: bool) -> bool {
        let Self::DatedPost { before, day, month } = self else {
            return false;
        };
        let completed = if complete_heading {
            self.with_section_heading(candidate)
        } else {
            Cow::Borrowed(candidate)
        };
        let (Ok(old), Ok(new)) = (
            unique_section(before, |label| dated(label, day, month)),
            unique_section(&completed, |label| dated(label, day, month)),
        ) else {
            return false;
        };
        let outside = |text: &str, section: &Range<usize>| {
            text[..section.start].trim().chars().count() + text[section.end..].trim().chars().count()
        };
        outside(&completed, &new) * 2 < outside(before, &old)
    }

    /// The heading lines of the sub-sections inside the target post's section
    /// of the accepted document (the `### Kilder` note under a post), in
    /// order. Told to the author on a section-only turn so it sends them back;
    /// empty for any other preservation, for a target that does not resolve,
    /// and for a post without sub-sections.
    /// At most six, each cut to 120 characters: they go into a prompt.
    pub(crate) fn subheadings(&self) -> Vec<String> {
        let Self::DatedPost { before, day, month } = self else {
            return Vec::new();
        };
        let Ok(target) = unique_heading_section(before, |label| dated(label, day, month)) else {
            return Vec::new();
        };
        inner_sections(before, &target)
            .iter()
            .take(6)
            .map(|section| before[section.range.clone()].lines().next().unwrap_or_default().trim().chars().take(120).collect())
            .collect()
    }

    /// Section-only output: why a section that lost its sub-sections is
    /// refused, or `None` when it did not.
    ///
    /// [`Self::apply`] replaces the target's whole section, sub-sections
    /// included, with the model's. A model that sends a post's heading and
    /// text alone, when the accepted post had a `### Kilder` note under it,
    /// thereby deletes the note without anyone having asked for that. Only
    /// the case where the accepted section had sub-sections and the model's
    /// has none at all is refused: a model that kept one and reworded or
    /// renamed another has restructured, which is its call. The reason quotes
    /// what was lost, because the repair that follows sees only the candidate,
    /// not the accepted document, and would otherwise have to invent it.
    /// `None` too when either side's target does not resolve: `apply` says so.
    pub(crate) fn dropped_subsections(&self, candidate: &str) -> Option<String> {
        let Self::DatedPost { before, day, month } = self else {
            return None;
        };
        let old = unique_heading_section(before, |label| dated(label, day, month)).ok()?;
        let new = unique_heading_section(candidate, |label| dated(label, day, month)).ok()?;
        let lost = inner_sections(before, &old);
        if lost.is_empty() || !inner_sections(candidate, &new).is_empty() {
            return None;
        }
        let first = lost.iter().map(|section| section.range.start).min()?;
        let note = before[first..old.range.end].trim_end();
        Some(format!(
            "The accepted post has a sub-section under its text, and the section you sent has none:\n\n{note}\n\nSend the revised post again with that sub-section kept under the revised text, and change what it says so that it matches the revised text. Do not remove it."
        ))
    }
}

/// The sections nested inside `target` in `text` (deeper heading level, inside
/// its range), in document order, that a Markdown `#` heading opens. A
/// standalone bold line such as a hook ("**Kjenner du deg igjen?**") is also a
/// (level 6) section to `sections()`, which the quoted internal note needs, but
/// it is part of a post's text and not a sub-section of it: counting it would
/// let a section that kept a hook line but lost its `### Kilder` note through,
/// and would quote the whole old post as the "lost" note.
fn inner_sections(text: &str, target: &Section) -> Vec<Section> {
    sections(text)
        .into_iter()
        .filter(|section| section.level > target.level && section.range.start > target.range.start && section.range.end <= target.range.end)
        .filter(|section| opened_by_hash_heading(text, section))
        .collect()
}

/// Whether `section` starts at a `#` heading line (inside a blockquote too),
/// as opposed to a standalone bold label.
fn opened_by_hash_heading(text: &str, section: &Section) -> bool {
    text[section.range.start..].trim_start().trim_start_matches('>').trim_start().starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn internal_source_table_survives_a_rewrite_exactly() {
        let before = "# Reply\nOld body\n\n## Intern merknad\n| K2 | Pickup is not confirmed |\n| K3 | Notify logistics |\n";
        let preservation = Preservation::from_prompt(
            "Gjør svaret kortere. Behold den interne kildeoversikten.",
            before,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            preservation
                .apply("# Reply\nWarm body\n\n## Intern merknad\nLogistics has been notified.\n")
                .unwrap(),
            before.replace("Old body", "Warm body")
        );
        assert!(preservation
            .apply("# Reply\nWarm body without the notes")
            .is_err());
    }

    #[test]
    fn quoted_bold_internal_notes_are_preserved_without_reformatting() {
        let before = "# Reply\nHei Nora\n\n> **Intern merknad (ikke del av kundesvaret)**\n> - K2: Henting er ikke bekreftet.\n> - K3: Varsling må utføres.\n";
        let preservation = Preservation::from_prompt(
            "Gjør svaret varmere. Behold den interne kildeoversikten.",
            before,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            preservation
                .apply("# Reply\nHei kjære Nora\n\n## Intern merknad\nNotes changed.\n")
                .unwrap(),
            before.replace("Hei Nora", "Hei kjære Nora")
        );
    }

    #[test]
    fn quoted_note_subheadings_do_not_end_the_preserved_container() {
        let before = "# Reply\nOld body\n\n> **⚠ Intern merknad**\n>\n> **Kildehenvisninger:**\n> - K2: No confirmed pickup.\n>\n> **Status:**\n> Draft only.\n\n## Footer\nOld footer\n";
        let candidate = before
            .replace("Old body", "Warm body")
            .replace("No confirmed pickup.", "Collected.")
            .replace("Draft only.", "Sent.")
            .replace("Old footer", "New footer");
        let preservation = Preservation::from_prompt("Behold den interne kildeoversikten.", before)
            .unwrap()
            .unwrap();
        assert_eq!(
            preservation.apply(&candidate).unwrap(),
            before
                .replace("Old body", "Warm body")
                .replace("Old footer", "New footer")
        );
    }
    #[test]
    fn one_dated_post_cannot_rewrite_the_email_or_other_posts() {
        let before = "# Plan\nOverview\n\n## LinkedIn 7. oktober\nFirst\n\n## LinkedIn 9. oktober\nSecond\n\n### Kilder\nOriginal\n\n## E-post\nKeep exactly\n";
        let preservation =
            Preservation::from_prompt("Gjør innlegget for 9. oktober mer konkret.", before)
                .unwrap()
                .unwrap();
        let candidate = before
            .replace("First", "Changed by accident")
            .replace("Second", "Concrete")
            .replace("Keep exactly", "Lost email");
        assert_eq!(
            preservation.apply(&candidate).unwrap(),
            before.replace("Second", "Concrete")
        );
        assert!(preservation
            .apply(&candidate.replace("9. oktober", "11. oktober"))
            .is_err());
        let date_heading = before.replace("LinkedIn 9. oktober", "9. oktober");
        let preservation =
            Preservation::from_prompt("Gjør innlegget for 9. oktober mer konkret.", &date_heading)
                .unwrap()
                .unwrap();
        assert_eq!(
            preservation
                .apply(
                    &date_heading
                        .replace("Second", "Concrete")
                        .replace("Keep exactly", "Lost")
                )
                .unwrap(),
            date_heading.replace("Second", "Concrete")
        );
    }
    #[test]
    fn quoted_file_instructions_do_not_change_revision_scope() {
        assert!(Preservation::from_prompt("Rewrite everything.\n\n--- VEDLEGG: instructions.md ---\nBehold den interne kildeoversikten.", "# All\nText").unwrap().is_none());
        assert!(Preservation::from_prompt(
            "Behold den interne kildeoversikten.",
            "# Intern merknad\na\n# Intern merknad\nb"
        )
        .is_err());
    }

    #[test]
    fn targeted_piece_resolves_only_a_recognized_single_dated_post() {
        let before = "# Plan\nOverview\n\n## LinkedIn 7. oktober\nFirst\n\n## LinkedIn 9. oktober\nSecond\n\n## E-post\nThird\n";
        let range = Preservation::targeted_piece("Gjør innlegget for 9. oktober mer konkret.", before)
            .unwrap()
            .unwrap();
        assert_eq!(&before[range], "## LinkedIn 9. oktober\nSecond\n\n");
        assert_eq!(
            Preservation::targeted_piece("Gjør hele dokumentet kortere.", before).unwrap(),
            None,
            "a whole-document instruction names no piece"
        );
        assert_eq!(
            Preservation::targeted_piece(
                "Gjør innlegget for 9. oktober mer konkret og oppdater e-posten.",
                before
            )
            .unwrap(),
            None,
            "a mixed instruction stays whole-document"
        );
        assert!(Preservation::targeted_piece("Gjør innlegget for 31. oktober mer konkret.", before).is_err());
    }

    #[test]
    fn mixed_revision_requests_do_not_discard_other_requested_edits() {
        assert!(Preservation::from_prompt(
            "Gjør innlegget for 9. oktober mer konkret og oppdater e-posten.",
            "## LinkedIn 9. oktober\nPost\n## E-post\nEmail"
        )
        .unwrap()
        .is_none());
    }

    // ---- Section-only output (`LatencyLevers::dated_post_section_only`) ----

    /// A document with awkward edges around the target: trailing spaces, a
    /// missing final newline in the last section, blank lines of different
    /// depth, a nested sub-heading inside the target and a standalone bold
    /// label in the neighbour.
    const AWKWARD: &str = "# Plan  \n\nOverview\t\n\n\n## LinkedIn 7. oktober\nFirst  \n\n## LinkedIn 9. oktober\nSecond\n\n### Kilder\nOriginal\n\n\n## E-post\n**Emne:** x\n\n**Brødtekst:**\nKeep exactly  ";
    const PROMPT_9: &str = "Gjør innlegget for 9. oktober mer konkret.";

    fn dated_post(before: &str) -> Preservation {
        Preservation::from_prompt(PROMPT_9, before).unwrap().unwrap()
    }

    #[test]
    fn a_section_fragment_splices_byte_identically_outside_the_section() {
        let preservation = dated_post(AWKWARD);
        let old = Preservation::targeted_piece(PROMPT_9, AWKWARD).unwrap().unwrap();
        let section = "## LinkedIn 9. oktober\nConcrete  \n\n### Kilder\nChanged source\n";
        for fragment in [
            section.to_owned(),
            format!("{section}\n\n\n"),
            section.trim_end().to_owned(),
            format!("Here is the section:\n\n{section}"),
            format!("\n\n{section}"),
        ] {
            let spliced = preservation.apply(&preservation.with_section_heading(&fragment)).unwrap();
            assert_eq!(&spliced[..old.start], &AWKWARD[..old.start], "before the section, byte for byte");
            let tail = spliced.len() - (AWKWARD.len() - old.end);
            assert_eq!(&spliced[tail..], &AWKWARD[old.end..], "after the section, byte for byte");
            let new_section = &spliced[old.start..tail];
            assert!(new_section.starts_with("## LinkedIn 9. oktober\nConcrete  \n"), "{new_section:?}");
            assert!(new_section.contains("Changed source"), "{new_section:?}");
            assert!(!new_section.contains("Original"), "{new_section:?}");
            assert_eq!(
                &new_section[new_section.trim_end().len()..],
                &AWKWARD[old.clone()][AWKWARD[old.clone()].trim_end().len()..],
                "the section's own boundary whitespace is the original's, whatever the fragment ended with"
            );
        }
        let last = "# Plan\n\n## LinkedIn 9. oktober\nSecond\n";
        let preservation = dated_post(last);
        assert_eq!(
            preservation.apply("## LinkedIn 9. oktober\nConcrete").unwrap(),
            "# Plan\n\n## LinkedIn 9. oktober\nConcrete\n",
            "a section that ends the document keeps the original's final newline"
        );
    }

    #[test]
    fn a_section_fragment_equals_the_same_section_inside_a_whole_document() {
        let preservation = dated_post(AWKWARD);
        let whole = AWKWARD
            .replace("First  ", "Lost by accident")
            .replace("Second", "Concrete")
            .replace("Original", "Changed source")
            .replace("Keep exactly  ", "Lost email");
        let range = Preservation::targeted_piece(PROMPT_9, &whole).unwrap().unwrap();
        let fragment = &whole[range];
        assert_eq!(
            preservation.apply(&preservation.with_section_heading(fragment)).unwrap(),
            preservation.apply(&whole).unwrap()
        );
        assert!(matches!(preservation.with_section_heading(&whole), Cow::Borrowed(_)), "a whole document is never rewritten");
        assert!(matches!(preservation.with_section_heading(fragment), Cow::Borrowed(_)), "nor a fragment that has its heading");
    }

    #[test]
    fn a_fragment_with_no_heading_gets_the_original_heading() {
        let before = "# Plan\n\n## LinkedIn — 9. oktober 2026\nSecond\n\n### Kilder\nOriginal\n\n## E-post\nKeep exactly\n";
        let preservation = dated_post(before);
        for fragment in [
            "Concrete body.\n",
            "Concrete body.",
            "\nConcrete body.\n\n",
            "**Status:** Utkast\n\nConcrete body.\n\n### Kilder\nChanged source\n",
        ] {
            assert!(preservation.apply(fragment).is_err(), "without the completion the section is not found: {fragment:?}");
            let completed = preservation.with_section_heading(fragment);
            assert!(completed.starts_with("## LinkedIn — 9. oktober 2026\n"), "{completed:?}");
            assert!(completed.ends_with(fragment), "the model's text follows the heading unchanged");
            let spliced = preservation.apply(&completed).unwrap();
            assert!(spliced.starts_with("# Plan\n\n## LinkedIn — 9. oktober 2026\n"), "{spliced:?}");
            assert!(spliced.ends_with("\n## E-post\nKeep exactly\n"), "{spliced:?}");
            assert!(spliced.contains("Concrete body."), "{spliced:?}");
            assert!(!spliced.contains("Second"), "{spliced:?}");
        }
        // The heading line is the original's byte for byte, level and dash included.
        let quoted = "# Plan\n\n> ## LinkedIn 9. oktober\n> Second\n\n## E-post\nx\n";
        let preservation = dated_post(quoted);
        assert!(preservation.with_section_heading("Concrete\n").starts_with("> ## LinkedIn 9. oktober\nConcrete"));
    }

    #[test]
    fn a_fragment_with_the_wrong_heading_is_refused_as_it_is_today() {
        let preservation = dated_post(AWKWARD);
        for fragment in [
            "## LinkedIn 11. oktober\nConcrete\n",
            "## LinkedIn 7. oktober\nConcrete\n",
            "# LinkedIn\nConcrete\n",
            "Intro line\n\n## E-post\nConcrete\n",
            "# Plan\n\n## LinkedIn 7. oktober\nFirst\n\n## E-post\nKeep\n",
        ] {
            assert!(matches!(preservation.with_section_heading(fragment), Cow::Borrowed(_)), "{fragment:?}");
            assert_eq!(
                preservation.apply(&preservation.with_section_heading(fragment)),
                preservation.apply(fragment),
                "{fragment:?}: the same refusal with the lever's completion as without it"
            );
            assert!(preservation.apply(&preservation.with_section_heading(fragment)).is_err(), "{fragment:?}");
        }
        // Two sections that both match the target stay ambiguous.
        let twice = "## LinkedIn 9. oktober\nA\n\n## LinkedIn 9. oktober\nB\n";
        assert!(preservation.apply(&preservation.with_section_heading(twice)).is_err());
    }

    #[test]
    fn a_fragment_with_several_sections_splices_by_the_one_that_matches() {
        let before = "# Plan\n\n## LinkedIn 7. oktober\nFirst\n\n## LinkedIn 9. oktober\nSecond\n\n## E-post\nKeep exactly\n";
        let preservation = dated_post(before);
        let fragment = "## LinkedIn 7. oktober\nChanged by accident\n\n## LinkedIn 9. oktober\nConcrete\n\n## E-post\nLost email\n";
        assert_eq!(
            preservation.apply(&preservation.with_section_heading(fragment)).unwrap(),
            before.replace("Second", "Concrete")
        );
        assert!(
            !preservation.is_section_fragment(fragment, true),
            "every other section came back too: that is a document, whatever else it left off"
        );
        // A neighbour or two beside the target is still a fragment when most of the document is left off.
        let preservation = dated_post(AWKWARD);
        let two = "## LinkedIn 7. oktober\nChanged by accident\n\n## LinkedIn 9. oktober\nConcrete\n";
        assert!(preservation.is_section_fragment(two, true));
        assert!(preservation.is_section_fragment(two, false), "with its heading it splices without the lever too");
        assert_eq!(
            preservation.apply(two).unwrap(),
            AWKWARD.replace("Second\n\n### Kilder\nOriginal", "Concrete"),
            "and it splices by the section that matches, the neighbour and the rest of the document untouched"
        );
    }

    #[test]
    fn only_a_dated_post_completes_a_fragment_or_counts_one() {
        let notes_before = "# Reply\nOld body\n\n## Intern merknad\nSources\n";
        let notes = Preservation::from_prompt("Gjør svaret kortere. Behold den interne kildeoversikten.", notes_before)
            .unwrap()
            .unwrap();
        assert!(matches!(notes.with_section_heading("Body only"), Cow::Borrowed("Body only")));
        assert!(!notes.is_section_fragment("Body only", true));
        assert!(!notes.is_section_fragment(notes_before, true));
    }

    #[test]
    fn a_section_fragment_is_told_apart_from_a_whole_document() {
        let preservation = dated_post(AWKWARD);
        let range = Preservation::targeted_piece(PROMPT_9, AWKWARD).unwrap().unwrap();
        let section = &AWKWARD[range];
        assert!(preservation.is_section_fragment(section, true), "the section alone");
        assert!(preservation.is_section_fragment(section, false), "spliced without the lever too");
        assert!(preservation.is_section_fragment("Concrete body only", true), "a section without its heading");
        assert!(!preservation.is_section_fragment("Concrete body only", false), "which only the lever can splice");
        assert!(preservation.is_section_fragment(&format!("\n\n{section}\n\n"), true));
        assert!(!preservation.is_section_fragment(AWKWARD, true), "the accepted document itself");
        assert!(!preservation.is_section_fragment(&AWKWARD.replace("Second", "Concrete"), true), "a whole rewritten document");
        assert!(!preservation.is_section_fragment(&AWKWARD.replace("First  ", "").replace("Keep exactly  ", ""), true),
            "a document that left a little out is still a document");
        assert!(!preservation.is_section_fragment("## LinkedIn 11. oktober\nWrong", true), "a section this revision cannot use is not counted");
        // A document that is nothing but the section has no outside to compare.
        let alone = "## LinkedIn 9. oktober\nSecond\n";
        assert!(!dated_post(alone).is_section_fragment("## LinkedIn 9. oktober\nConcrete\n", true));
    }

    #[test]
    fn a_section_fragment_splices_the_same_in_a_document_with_windows_line_endings() {
        let before = "# Plan\r\n\r\n## LinkedIn 9. oktober\r\nSecond\r\n\r\n## E-post\r\nKeep exactly\r\n";
        let preservation = dated_post(before);
        let spliced = preservation
            .apply(&preservation.with_section_heading("Concrete\r\nMore\r\n"))
            .unwrap();
        assert_eq!(spliced, "# Plan\r\n\r\n## LinkedIn 9. oktober\r\nConcrete\r\nMore\r\n\r\n## E-post\r\nKeep exactly\r\n");
        let with_heading = preservation
            .apply(&preservation.with_section_heading("## LinkedIn 9. oktober\nConcrete\nMore\n"))
            .unwrap();
        assert_eq!(with_heading, "# Plan\r\n\r\n## LinkedIn 9. oktober\nConcrete\nMore\r\n\r\n## E-post\r\nKeep exactly\r\n");
    }

    // ---- The recorded corpus ----

    /// One recorded journey-03 follow-up: the revision instruction the user
    /// sent, the campaign document as first accepted and as revised.
    struct RecordedPair {
        take: String,
        prompt: String,
        initial: String,
        revised: String,
    }

    /// Every (initial, revised) journey-03 pair in the Q16e, Q16f and Q16h
    /// recordings, read-only (Q16h ran with section-only output on, and three of
    /// its revisions lost the post's note). The Playwright report of each take carries the
    /// thread's transcripts as base64 attachments; the documents are the
    /// artifacts of the first and the last assistant turn. The root is
    /// `VEREVON_RECORDINGS_DIR` when set, else `C:/private/verevon-recordings`.
    /// Empty when the recordings are not on this machine.
    fn recorded_pairs() -> Vec<RecordedPair> {
        use base64::Engine as _;
        let root = std::path::PathBuf::from(
            std::env::var("VEREVON_RECORDINGS_DIR").unwrap_or_else(|_| "C:/private/verevon-recordings".to_owned()),
        );
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut roots: Vec<_> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                (name.starts_with("q16e-") || name.starts_with("q16f-") || name.starts_with("q16h-")) && entry.path().is_dir()
            })
            .map(|entry| entry.path())
            .collect();
        roots.sort();
        fn attachments(value: &serde_json::Value, found: &mut std::collections::BTreeMap<String, String>) {
            match value {
                serde_json::Value::Object(map) => {
                    if let Some(items) = map.get("attachments").and_then(serde_json::Value::as_array) {
                        for item in items {
                            if let (Some(name), Some(body)) = (item["name"].as_str(), item["body"].as_str()) {
                                found.insert(name.to_owned(), body.to_owned());
                            }
                        }
                    }
                    map.values().for_each(|value| attachments(value, found));
                }
                serde_json::Value::Array(items) => items.iter().for_each(|value| attachments(value, found)),
                _ => {}
            }
        }
        let decode = |found: &std::collections::BTreeMap<String, String>, name: &str| -> Option<serde_json::Value> {
            let bytes = base64::engine::general_purpose::STANDARD.decode(found.get(name)?).ok()?;
            serde_json::from_slice(&bytes).ok()
        };
        let last_artifact = |transcript: &serde_json::Value| -> Option<String> {
            transcript["turns"].as_array()?.iter().rev().find_map(|turn| {
                turn["artifacts"].as_array()?.last()?["content"].as_str().map(str::to_owned)
            })
        };
        let mut pairs = Vec::new();
        for root in roots {
            let mut takes: Vec<_> = std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    name.starts_with("03-") && name.ends_with(".json") && !name.ends_with(".run.json")
                })
                .collect();
            takes.sort();
            for path in takes {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(report) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                let mut found = std::collections::BTreeMap::new();
                attachments(&report, &mut found);
                let (Some(initial), Some(revised)) = (decode(&found, "initial-transcript"), decode(&found, "revised-transcript")) else {
                    continue;
                };
                let prompt = revised["turns"].as_array().and_then(|turns| {
                    turns.iter().rev().find(|turn| turn["role"] == "user").and_then(|turn| turn["content"].as_str())
                });
                if let (Some(prompt), Some(first), Some(last)) = (prompt, last_artifact(&initial), last_artifact(&revised)) {
                    pairs.push(RecordedPair {
                        take: format!(
                            "{}/{}",
                            root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                            path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
                        ),
                        prompt: prompt.to_owned(),
                        initial: first,
                        revised: last,
                    });
                }
            }
        }
        pairs
    }

    /// The recorded journey-03 revisions, replayed as section-only output.
    /// For each recorded (initial, revised) pair, the section of the revised
    /// document is applied to the initial document as a fragment, with its
    /// heading and with the heading line left off, and must give exactly what
    /// `Preservation::apply` gives from the whole revised document; the bytes
    /// outside the target are the initial's.
    #[test]
    fn every_recorded_journey_03_revision_splices_the_same_from_its_section_alone() {
        let pairs = recorded_pairs();
        if pairs.is_empty() {
            eprintln!("recorded journeys not found on this machine: corpus check skipped");
            return;
        }
        let mut headingless = 0;
        for pair in &pairs {
            let take = &pair.take;
            let preservation = Preservation::from_prompt(&pair.prompt, &pair.initial)
                .unwrap_or_else(|error| panic!("{take}: {error}"))
                .unwrap_or_else(|| panic!("{take}: the recorded instruction is a dated-post revision"));
            let whole = preservation.apply(&pair.revised).unwrap_or_else(|error| panic!("{take}: {error}"));
            let old = Preservation::targeted_piece(&pair.prompt, &pair.initial).unwrap().unwrap();
            let section = Preservation::targeted_piece(&pair.prompt, &pair.revised)
                .unwrap_or_else(|error| panic!("{take}: {error}"))
                .unwrap_or_else(|| panic!("{take}: the revised document has the post"));
            let fragment = &pair.revised[section];

            let spliced = preservation
                .apply(&preservation.with_section_heading(fragment))
                .unwrap_or_else(|error| panic!("{take}: {error}"));
            assert_eq!(spliced, whole, "{take}: the section alone splices as the whole revised document does");
            assert_eq!(&spliced[..old.start], &pair.initial[..old.start], "{take}: bytes before the post");
            assert!(
                spliced.ends_with(&pair.initial[old.end..]),
                "{take}: bytes after the post"
            );
            assert!(preservation.is_section_fragment(fragment, true), "{take}: the section alone is a fragment");
            assert!(preservation.is_section_fragment(fragment, false), "{take}: and is spliced without the lever too");
            assert!(!preservation.is_section_fragment(&pair.revised, true), "{take}: the whole revised document is not");
            assert!(!preservation.is_section_fragment(&pair.revised, false), "{take}: on either setting");

            // The same fragment with its heading line left off, when the
            // revision kept the initial's heading: the gateway puts it back.
            let fragment_heading = fragment.split_inclusive('\n').next().unwrap_or_default();
            let initial_heading = pair.initial[old.start..].split_inclusive('\n').next().unwrap_or_default();
            if fragment_heading == initial_heading {
                headingless += 1;
                let body = &fragment[fragment_heading.len()..];
                assert_eq!(
                    preservation.apply(&preservation.with_section_heading(body)).unwrap_or_else(|error| panic!("{take}: {error}")),
                    whole,
                    "{take}: a fragment without its heading line splices the same"
                );
            }
        }
        eprintln!("{} recorded journey-03 pairs replayed as fragments ({headingless} also without the heading line)", pairs.len());
    }

    #[test]
    fn internal_heading_counts_tell_no_note_from_two_notes() {
        assert_eq!(internal_heading_counts("# Svar\nHei\n"), (0, 1));
        assert_eq!(internal_heading_counts("# Svar\nHei\n\n## Intern merknad\nKilder\n"), (1, 2));
        assert_eq!(internal_heading_counts("# Svar\nHei\n\n## Intern merknad\nA\n\n### Interne kilder\nB\n"), (2, 3), "a note plus a sub-note: not unique");
        assert_eq!(internal_heading_counts("# Svar\nHei\n\n## Kilder\nA\n"), (0, 2), "a note called something else");
        // Only a unique internal heading gives a body end, which is why two are reported as missing sections.
        assert!(internal_notes_start("# Svar\nHei\n\n## Intern merknad\nA\n\n### Interne kilder\nB\n").is_none());
    }

    // ---- A section that lost its sub-sections (`dropped_subsections`) ----

    const WITH_NOTES: &str = "# Plan\n\nOverview\n\n## LinkedIn 7. oktober\nFirst\n\n### Kilder\nFirst note\n\n## LinkedIn 9. oktober\nSecond\n\n### Kilder\n- Produktinformasjon brukt: arm.\n- Bygger på briefen.\n\n## E-post\nKeep exactly\n";

    #[test]
    fn the_subheadings_of_the_target_post_are_listed_in_order() {
        assert_eq!(dated_post(WITH_NOTES).subheadings(), ["### Kilder"]);
        assert!(dated_post(AWKWARD).subheadings() == ["### Kilder"], "a sub-heading inside the target counts even beside other sections");
        let bare = "# Plan\n\n## LinkedIn 9. oktober\nSecond\n\n## E-post\nKeep\n";
        assert!(dated_post(bare).subheadings().is_empty(), "a post without sub-sections lists none");
        let two = "# Plan\n\n## LinkedIn 9. oktober\nSecond\n\n### Vinkel\nA\n\n### Kilder\nB\n\n## E-post\nKeep\n";
        assert_eq!(dated_post(two).subheadings(), ["### Vinkel", "### Kilder"]);
        let notes = Preservation::from_prompt("Gjør svaret kortere. Behold den interne kildeoversikten.", "# Reply\nBody\n\n## Intern merknad\nSources\n")
            .unwrap()
            .unwrap();
        assert!(notes.subheadings().is_empty(), "only a dated post has any");
    }

    #[test]
    fn a_post_that_comes_back_without_its_note_is_refused_with_the_note_quoted() {
        let preservation = dated_post(WITH_NOTES);
        // Q16h take 1: the heading, the status line and the text, nothing under them.
        for fragment in [
            "## LinkedIn 9. oktober\nConcrete\n",
            "Concrete\n",
            "## LinkedIn 9. oktober\n**Status:** Utkast\n\nConcrete #tag\n\n## E-post\nLost email\n",
            // A sub-section somewhere else in the candidate is not this post's note.
            "## LinkedIn 9. oktober\nConcrete\n\n## E-post\nText\n\n### Kilder\nThe e-mail's own note.\n",
            // A standalone bold line is a hook in the text, not a sub-section: it does not stand in for the note.
            "## LinkedIn 9. oktober\n**Kjenner du deg igjen?**\nConcrete\n",
            // A bold label in place of the heading is a rename the repair may ask to undo.
            "## LinkedIn 9. oktober\nConcrete\n\n**Kilder**\nEn linje.\n",
        ] {
            let completed = preservation.with_section_heading(fragment);
            let reason = preservation.dropped_subsections(&completed).unwrap_or_else(|| panic!("{fragment:?} should be refused"));
            assert!(reason.contains("### Kilder\n- Produktinformasjon brukt: arm.\n- Bygger på briefen."), "the lost note is quoted: {reason}");
            assert!(!reason.contains("First note"), "and only the target post's, not a neighbour's: {reason}");
            assert!(!reason.contains("Keep exactly"), "nor what follows the post: {reason}");
        }
    }

    #[test]
    fn a_post_that_keeps_a_sub_section_is_not_refused() {
        let preservation = dated_post(WITH_NOTES);
        for fragment in [
            "## LinkedIn 9. oktober\nConcrete\n\n### Kilder\n- Produktinformasjon brukt: arm; knapp.\n",
            "## LinkedIn 9. oktober\nConcrete\n\n### Kilder\n- Produktinformasjon brukt: arm.\n- Bygger på briefen.\n",
            // Another name: a restructure is the model's call.
            "## LinkedIn 9. oktober\nConcrete\n\n### Kilder og notater\nEn linje.\n",
            // A whole document with its notes.
            WITH_NOTES.replace("Second", "Concrete").as_str(),
        ] {
            assert_eq!(preservation.dropped_subsections(&preservation.with_section_heading(fragment)), None, "{fragment:?}");
        }
        // Nothing to lose: the accepted post had no sub-section.
        let bare = "# Plan\n\n## LinkedIn 9. oktober\nSecond\n\n## E-post\nKeep\n";
        assert_eq!(dated_post(bare).dropped_subsections("## LinkedIn 9. oktober\nConcrete\n"), None);
        // A wrong or ambiguous heading is `apply`'s to refuse, with its own reason.
        for fragment in ["## LinkedIn 11. oktober\nConcrete\n", "## LinkedIn 9. oktober\nA\n\n## LinkedIn 9. oktober\nB\n"] {
            assert_eq!(preservation.dropped_subsections(fragment), None, "{fragment:?}");
            assert!(preservation.apply(fragment).is_err(), "{fragment:?}");
        }
        // Another preservation is untouched.
        let notes = Preservation::from_prompt("Gjør svaret kortere. Behold den interne kildeoversikten.", "# Reply\nBody\n\n## Intern merknad\nSources\n")
            .unwrap()
            .unwrap();
        assert_eq!(notes.dropped_subsections("# Reply\nShort\n"), None);
    }

    /// A standalone bold line in the ACCEPTED post is a hook in its text. It is
    /// neither listed to the author as a sub-section nor quoted as a lost note,
    /// and a rewrite without it is never refused for it.
    #[test]
    fn a_bold_hook_line_in_the_accepted_post_is_not_a_sub_section() {
        let hooked = "# Plan\n\n## LinkedIn 9. oktober\n**Kjenner du deg igjen?**\nSecond\n\n## E-post\nKeep\n";
        let preservation = dated_post(hooked);
        assert!(preservation.subheadings().is_empty(), "{:?}", preservation.subheadings());
        assert_eq!(preservation.dropped_subsections("## LinkedIn 9. oktober\nConcrete\n"), None, "nothing to lose");
        // With a real note as well, only the note is listed and quoted.
        let both = "# Plan\n\n## LinkedIn 9. oktober\n**Kjenner du deg igjen?**\nSecond\n\n### Kilder\n- Bygger på briefen.\n\n## E-post\nKeep\n";
        let preservation = dated_post(both);
        assert_eq!(preservation.subheadings(), ["### Kilder"]);
        let reason = preservation.dropped_subsections("## LinkedIn 9. oktober\n**Kjenner du deg igjen?**\nConcrete\n").expect("the hook does not replace the note");
        assert!(reason.contains("### Kilder\n- Bygger på briefen."), "{reason}");
        assert!(!reason.contains("Second") && !reason.contains("Kjenner du deg igjen"), "only the note is quoted: {reason}");
    }

    /// The same on a document with Windows line endings: the listed heading has
    /// no carriage return, and the lost note is found and quoted.
    #[test]
    fn sub_sections_are_found_in_a_document_with_windows_line_endings() {
        let crlf = WITH_NOTES.replace('\n', "\r\n");
        let preservation = dated_post(&crlf);
        assert_eq!(preservation.subheadings(), ["### Kilder"]);
        let reason = preservation
            .dropped_subsections("## LinkedIn 9. oktober\r\nConcrete\r\n")
            .expect("a section without its note is refused on CRLF too");
        assert!(reason.contains("### Kilder"), "{reason}");
        assert!(reason.contains("Produktinformasjon brukt: arm."), "{reason}");
        assert_eq!(
            preservation.dropped_subsections("## LinkedIn 9. oktober\r\nConcrete\r\n\r\n### Kilder\r\n- Bygger på briefen.\r\n"),
            None
        );
    }

    /// A request that also takes the note (or anything else) away is a second
    /// edit: it is not a dated-post revision, so the refusal never applies.
    #[test]
    fn asking_for_the_note_to_go_keeps_the_request_on_the_whole_document_path() {
        for prompt in [
            "Gjør innlegget for 9. oktober kortere og fjern kildene.",
            "Gjør innlegget for 9. oktober kortere og slett kildenoten.",
            "Gjør innlegget for 9. oktober kortere og ta bort kilden.",
            "Gjør innlegget for 9. oktober kortere uten kilder.",
            "Revise the post for 9. oktober and remove the sources.",
            "Revise the post for 9. oktober and delete the note.",
            "Revise the post for 9. oktober without the source note.",
        ] {
            assert!(Preservation::from_prompt(prompt, WITH_NOTES).unwrap().is_none(), "{prompt}");
        }
        assert!(Preservation::from_prompt(PROMPT_9, WITH_NOTES).unwrap().is_some(), "the plain request still is");
    }

    /// The recorded revisions, where the accepted post had a `### ` note: the
    /// refusal must fire exactly for those whose revised post has none left,
    /// counted here by a plain line scan rather than by the parser under test.
    #[test]
    fn the_refusal_fires_on_the_recorded_revisions_that_lost_their_note_and_no_other() {
        let pairs = recorded_pairs();
        if pairs.is_empty() {
            eprintln!("recorded journeys not found on this machine: corpus check skipped");
            return;
        }
        let notes_under = |document: &str, range: Range<usize>| document[range].lines().skip(1).filter(|line| line.starts_with("### ")).count();
        let (mut with_note, mut lost) = (0usize, 0usize);
        for pair in &pairs {
            let take = &pair.take;
            let preservation = Preservation::from_prompt(&pair.prompt, &pair.initial).unwrap().unwrap();
            let old = Preservation::targeted_piece(&pair.prompt, &pair.initial).unwrap().unwrap();
            let new = Preservation::targeted_piece(&pair.prompt, &pair.revised).unwrap().unwrap();
            let fragment = pair.revised[new.clone()].to_owned();
            let had = notes_under(&pair.initial, old);
            let has = notes_under(&pair.revised, new);
            if had > 0 { with_note += 1; }
            let refused = preservation.dropped_subsections(&preservation.with_section_heading(&fragment)).is_some();
            assert_eq!(refused, had > 0 && has == 0, "{take}: had {had} sub-headings, the revised post has {has}");
            if refused { lost += 1; }
        }
        eprintln!("{} recorded revisions; {with_note} had a note under the post; {lost} lost it and are refused", pairs.len());
    }
}
