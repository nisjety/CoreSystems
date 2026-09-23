//! Copy protected Markdown sections from the accepted artifact, byte for byte.
//! This intentionally supports a small set of unambiguous revision commands;
//! it is not a claim to understand every natural-language preservation request.

use std::ops::Range;

#[derive(Clone, Debug)]
struct Section {
    label: String,
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
    let found: Vec<_> = sections(text)
        .into_iter()
        .filter(|section| matches(&section.label))
        .collect();
    if found.len() != 1 {
        return Err("The requested section must have one distinct Markdown heading; do not remove, duplicate or rename it.".into());
    }
    Ok(found[0].range.clone())
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
                "and update",
                "and change",
                "and revise",
                "and the post",
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
    fn mixed_revision_requests_do_not_discard_other_requested_edits() {
        assert!(Preservation::from_prompt(
            "Gjør innlegget for 9. oktober mer konkret og oppdater e-posten.",
            "## LinkedIn 9. oktober\nPost\n## E-post\nEmail"
        )
        .unwrap()
        .is_none());
    }
}
