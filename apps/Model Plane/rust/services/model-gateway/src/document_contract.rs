//! Local checks for explicit campaign-copy requirements adopted by the user.
//! This is a bounded grammar, not an authority extractor for arbitrary files.
use crate::result_validation::{content_hash, count_words};
use crate::source_validation::SourceContext;
use mp_contracts::model_plane::v1::ChatMessage;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct CampaignContract {
    posts: Option<(usize, usize)>,
    email: Option<(usize, usize)>,
    post_count: Option<usize>,
    plural: bool,
    no_urls: bool,
    no_emoji: bool,
    hashtags: Option<usize>,
    prohibited: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyCheck {
    pub section: String,
    pub body_hash: String,
    pub words: usize,
    pub minimum: usize,
    pub maximum: usize,
}

/// Exact allowance for the changed prose across one copy section. Multiple
/// failed paragraphs share this budget; each must not consume it independently.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairWordBudget {
    section: String,
    indexes: Vec<usize>,
    current_body_words: usize,
    unchanged_body_words: usize,
    minimum_replacement_body_words: usize,
    maximum_replacement_body_words: usize,
    target_replacement_body_words: usize,
}

#[derive(Debug)]
pub struct SectionRepair {
    pub range: std::ops::Range<usize>,
    pub reason: String,
}

#[derive(Debug)]
pub struct ContractFailure {
    pub message: String,
    /// Empty when structure/whole-document errors make section binding unsafe.
    pub sections: Vec<SectionRepair>,
}

fn number(token: &str) -> Option<usize> {
    match token.trim_matches(|ch: char| !ch.is_alphanumeric()) {
        "en" | "ett" | "én" | "one" => Some(1),
        "to" | "two" => Some(2),
        "tre" | "three" => Some(3),
        "fire" | "four" => Some(4),
        "fem" | "five" => Some(5),
        value => value.parse().ok(),
    }
}

fn word_range(line: &str) -> Option<(usize, usize)> {
    let normalized = line.replace(['–', '—'], "-");
    let words: Vec<_> = normalized.split_whitespace().collect();
    words.windows(2).find_map(|pair| {
        if !matches!(
            pair[1].trim_matches(|ch: char| !ch.is_alphabetic()),
            "ord" | "words"
        ) {
            return None;
        }
        let (lo, hi) = pair[0].split_once('-')?;
        let bounds = (lo.parse::<usize>().ok()?, hi.parse::<usize>().ok()?);
        (bounds.0 > 0 && bounds.0 <= bounds.1 && bounds.1 <= 1000).then_some(bounds)
    })
}

fn instruction(message: &ChatMessage) -> &str {
    message
        .content
        .split("\n\n--- VEDLEGG: ")
        .next()
        .unwrap_or_default()
}

impl CampaignContract {
    pub fn from_messages(messages: &[ChatMessage], sources: &SourceContext) -> Option<Self> {
        let requests: Vec<_> = messages
            .iter()
            .filter(|m| m.role == "user")
            .map(instruction)
            .collect();
        let adopted = requests.iter().any(|request| {
            let text = request.to_lowercase();
            ["bruk ", "use ", "follow ", "følg "]
                .iter()
                .any(|verb| text.starts_with(verb))
                && ["brief", "språkprofil", "language profile"]
                    .iter()
                    .any(|term| text.contains(term))
                && (text.contains("kampanje")
                    || text.contains("campaign")
                    || text.contains("linkedin"))
        });
        if !adopted {
            return None;
        }
        // A later explicit change of length/style needs a new section contract;
        // do not silently enforce the old one against that newer instruction.
        if requests.len() > 1
            && requests.last().is_some_and(|request| {
                let text = request.to_lowercase();
                [" ord", " words", "du-form", "singular", "«du»"]
                    .iter()
                    .any(|term| text.contains(term))
            })
        {
            return None;
        }
        let mut contract = Self {
            posts: None,
            email: None,
            post_count: None,
            plural: false,
            no_urls: false,
            no_emoji: false,
            hashtags: None,
            prohibited: Vec::new(),
        };
        for line in sources
            .sources
            .iter()
            .flat_map(|source| source.content.lines())
        {
            let lower = line.to_lowercase();
            if lower.contains("linkedin") {
                if let Some(range) = word_range(&lower) {
                    contract.posts = Some(range);
                    let words: Vec<_> = lower.split_whitespace().collect();
                    contract.post_count = words.windows(2).find_map(|pair| {
                        matches!(pair[1].trim_end_matches([',', '.']), "innlegg" | "posts")
                            .then(|| number(pair[0]))
                            .flatten()
                    });
                }
                if lower.contains("emneknagg") || lower.contains("hashtag") {
                    let words: Vec<_> = lower.split_whitespace().collect();
                    contract.hashtags = words.windows(2).find_map(|pair| {
                        matches!(pair[0], "maks" | "max" | "maximum")
                            .then(|| number(pair[1]))
                            .flatten()
                    });
                }
            }
            if lower.contains("e-post") || lower.contains("email") || lower.contains("e-mail") {
                if let Some(range) = word_range(&lower) {
                    contract.email = Some(range);
                }
            }
            contract.plural |=
                lower.contains("snakk til «dere»") || lower.contains("address readers as \"dere\"");
            contract.no_urls |=
                lower.contains("ikke oppgi nettadresse") || lower.contains("no url");
            contract.no_emoji |= lower.contains("ingen emoj") || lower.contains("no emoji");
            if lower.contains("unngå") || lower.contains("avoid") {
                for part in lower.split('«').skip(1) {
                    if let Some((phrase, _)) = part.split_once('»') {
                        if !phrase.is_empty() && phrase.len() < 100 {
                            contract.prohibited.push(phrase.to_owned());
                        }
                    }
                }
            }
        }
        (contract.posts.is_some() || contract.email.is_some()).then_some(contract)
    }

    pub fn guidance(&self) -> String {
        let target = |range: Option<(usize, usize)>| range.map(|(minimum, maximum)| minimum + (maximum - minimum) / 2);
        format!("The adopted campaign brief has local body checks: LinkedIn {:?}, email {:?}, expected posts {:?}. Draft toward these BODY-ONLY word targets: LinkedIn {:?}, email {:?}. Headings, dates, subject, preview text and source notes do not count toward those body targets. Use one ## LinkedIn heading per post (include its date), and one ## E-post/Email heading. Put metadata above the prose. Label the email body 'Brødtekst:' or 'Body:'. Put internal source notes under a separate ### Kilder/Sources heading. Do not include estimated word-count labels. Use only sourced facts and relevant invitations, never invented benefits or repetitive padding. If the brief requests a relatable situation, frame undocumented situations as questions or explicit possibilities; do not invent actual customer experiences, recipient circumstances or a product effect. Then state the documented feature directly, without claiming that it solves a problem or produces an undocumented benefit. Keep the exact body inside the requested bounds after every repair. Plural address required: {}. No URLs: {}. No emoji: {}. Hashtag maximum: {:?}. Prohibited wording: {:?}.",
            self.posts, self.email, self.post_count, target(self.posts), target(self.email), self.plural, self.no_urls, self.no_emoji, self.hashtags, self.prohibited)
    }

    pub(crate) fn repair_budgets(&self, candidate: &str, parts: &[&str], failed: &[usize]) -> Vec<RepairWordBudget> {
        copy_sections(candidate).into_iter().filter_map(|(heading, content)| {
            let post = post_heading(&heading);
            let (minimum, maximum) = if post { self.posts } else { self.email }?;
            let section_start = content.as_ptr() as usize - candidate.as_ptr() as usize;
            let section_end = section_start + content.len();
            let mut ranges = Vec::new();
            let mut indexes = Vec::new();
            for &index in failed {
                let part = parts.get(index)?;
                let start = part.as_ptr() as usize - candidate.as_ptr() as usize;
                let end = start + part.len();
                let overlap_start = start.max(section_start);
                let overlap_end = end.min(section_end);
                if overlap_start < overlap_end {
                    indexes.push(index);
                    ranges.push(overlap_start - section_start..overlap_end - section_start);
                }
            }
            if indexes.is_empty() { return None; }
            ranges.sort_by_key(|range| range.start);
            let mut unchanged = content.to_owned();
            for range in ranges.into_iter().rev() { unchanged.replace_range(range, ""); }
            let unchanged_body_words = count_words(&body_text(&unchanged, !post));
            Some(RepairWordBudget {
                section: heading,
                indexes,
                current_body_words: count_words(&body_text(content, !post)),
                unchanged_body_words,
                minimum_replacement_body_words: minimum.saturating_sub(unchanged_body_words),
                maximum_replacement_body_words: maximum.saturating_sub(unchanged_body_words),
                target_replacement_body_words: (minimum + (maximum - minimum) / 2).saturating_sub(unchanged_body_words),
            })
        }).collect()
    }

    pub fn validate(&self, candidate: &str) -> Result<Vec<BodyCheck>, String> {
        self.validate_scoped(candidate)
            .map_err(|failure| failure.message)
    }

    pub fn validate_scoped(&self, candidate: &str) -> Result<Vec<BodyCheck>, ContractFailure> {
        let sections = copy_sections(candidate);
        let post_count = sections
            .iter()
            .filter(|(heading, _)| post_heading(heading))
            .count();
        if let Some(expected) = self.post_count {
            if post_count != expected {
                return Err(ContractFailure {
                    message: format!(
                        "Expected {expected} distinct LinkedIn post headings; found {post_count}."
                    ),
                    sections: Vec::new(),
                });
            }
        }
        let mut email_count = 0;
        let mut checks = Vec::new();
        let mut failures = Vec::new();
        let mut repairs = Vec::new();
        if candidate.lines().any(|line| {
            let plain = line.trim().trim_matches(['*', '`', '(', ')']);
            plain.split_once(' ').is_some_and(|(n, unit)| {
                n.parse::<usize>().is_ok() && matches!(unit, "ord" | "words")
            })
        }) {
            failures.push(
                "Remove author-written word-count labels; the runtime records exact final counts."
                    .into(),
            );
        }
        let global_failure = !failures.is_empty();
        for (heading, content) in sections {
            let prior_failures = failures.len();
            let post = post_heading(&heading);
            let bounds = if post {
                self.posts
            } else {
                email_count += 1;
                self.email
            };
            let Some((minimum, maximum)) = bounds else {
                continue;
            };
            let body = body_text(content, !post);
            let words = count_words(&body);
            if !(minimum..=maximum).contains(&words) {
                failures.push(format!(
                    "{heading}: body has {words} words, needs {minimum}–{maximum}."
                ));
            }
            let lower = body.to_lowercase();
            if self.plural
                && lower
                    .split(|ch: char| !ch.is_alphabetic())
                    .any(|word| matches!(word, "du" | "deg" | "din" | "ditt" | "dine"))
            {
                failures.push(format!(
                    "{heading}: use plural dere/deres, not singular address."
                ));
            }
            if self.no_urls
                && ["https:", "http:", "www."]
                    .iter()
                    .any(|url| lower.contains(url))
            {
                failures.push(format!("{heading}: remove web addresses."));
            }
            if self.no_emoji
                && body
                    .chars()
                    .any(|ch| matches!(ch as u32, 0x1f000..=0x1faff | 0x2600..=0x27bf))
            {
                failures.push(format!("{heading}: remove emoji."));
            }
            if post
                && self.hashtags.is_some_and(|max| {
                    body.split_whitespace()
                        .filter(|word| word.starts_with('#'))
                        .count()
                        > max
                })
            {
                failures.push(format!("{heading}: too many hashtags."));
            }
            for phrase in &self.prohibited {
                if lower.contains(phrase) {
                    failures.push(format!("{heading}: remove prohibited wording {phrase:?}."));
                }
            }
            if failures.len() > prior_failures {
                let content = content.trim();
                let start = content.as_ptr() as usize - candidate.as_ptr() as usize;
                repairs.push(SectionRepair {
                    range: start..start + content.len(),
                    reason: format!("{} Replace only this section's content; preserve its metadata and source notes, and keep its body within {minimum}–{maximum} words. Do not add another deliverable or rename its heading.", failures[prior_failures..].join(" ")),
                });
            }
            checks.push(BodyCheck {
                section: heading,
                body_hash: content_hash(&body),
                words,
                minimum,
                maximum,
            });
        }
        if self.email.is_some() && email_count != 1 {
            repairs.clear();
            failures.push("Expected exactly one distinct E-post/Email heading.".into());
        }
        if self.posts.is_some() && post_count == 0 {
            repairs.clear();
            failures.push("Missing LinkedIn post headings.".into());
        }
        if failures.is_empty() {
            Ok(checks)
        } else {
            Err(ContractFailure {
                message: failures.join("\n"),
                sections: if global_failure { Vec::new() } else { repairs },
            })
        }
    }
}

fn post_heading(heading: &str) -> bool {
    heading.contains("linkedin") || heading.contains("innlegg") || heading.starts_with("post ")
}
fn copy_heading(heading: &str) -> bool {
    post_heading(heading)
        || ["e-post", "email", "e-mail"]
            .iter()
            .any(|term| heading.contains(term))
}
fn copy_sections(candidate: &str) -> Vec<(String, &str)> {
    let mut headings = Vec::new();
    let mut offset = 0;
    for line in candidate.split_inclusive('\n') {
        let trimmed = line.trim();
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        if (1..=6).contains(&level) && trimmed.as_bytes().get(level) == Some(&b' ') {
            headings.push((
                offset,
                offset + line.len(),
                level,
                trimmed[level..].trim().to_lowercase(),
            ));
        }
        offset += line.len();
    }
    headings
        .iter()
        .enumerate()
        .filter_map(|(index, (_, start, level, heading))| {
            if !copy_heading(heading) {
                return None;
            }
            let end = headings[index + 1..]
                .iter()
                .find(|(_, _, next, _)| next <= level)
                .map_or(candidate.len(), |(start, ..)| *start);
            // A document/group title may mention its channels. Only the
            // actual copy sections count as pieces, not their container too.
            if headings[index + 1..]
                .iter()
                .take_while(|(start, ..)| *start < end)
                .any(|(_, _, nested_level, nested)| nested_level > level && copy_heading(nested))
            {
                return None;
            }
            Some((heading.clone(), &candidate[*start..end]))
        })
        .collect()
}

fn body_text(content: &str, email: bool) -> String {
    let normalized = content.replace(['*', '`'], "");
    let mut content = normalized.as_str();
    if email {
        if let Some(index) = content
            .split_inclusive('\n')
            .scan(0, |offset, line| {
                let start = *offset;
                *offset += line.len();
                Some((start, line.to_lowercase()))
            })
            .find_map(|(start, line)| {
                ["brødtekst:", "body:", "e-posttekst:"]
                    .iter()
                    .find_map(|label| {
                        line.trim_start()
                            .trim_start_matches('#')
                            .trim_start()
                            .strip_prefix(label)
                            .map(|_| start + line.find(':').unwrap() + 1)
                    })
            })
        {
            content = &content[index..];
        }
    }
    let mut lines = Vec::new();
    for line in content.lines() {
        let lower = line.trim().trim_start_matches('#').trim().to_lowercase();
        if ["kilder", "sources", "intern merknad", "kildenoter"]
            .iter()
            .any(|label| lower.starts_with(label))
            || (line.trim().starts_with('#')
                && [
                    "kildegrunnlag",
                    "produktgrunnlag",
                    "faktagrunnlag",
                    "produktinformasjon",
                    "bygger på",
                ]
                .iter()
                .any(|label| lower.starts_with(label)))
        {
            break;
        }
        if line.trim().starts_with('#') && line.trim().starts_with("##") {
            continue;
        }
        if lower.is_empty() || lower.chars().all(|ch| matches!(ch, '-' | '_')) {
            continue;
        }
        if [
            "vinkel",
            "angle",
            "emne",
            "subject",
            "preview",
            "forhåndsvisning",
            "preheader",
            "produktgrunnlag",
            "produktinformasjon",
            "kildegrunnlag",
            "faktagrunnlag",
            "bygger på",
            "ordtelling",
            "antall ord",
        ]
        .iter()
        .any(|label| lower.starts_with(label) && lower.contains(':'))
        {
            continue;
        }
        if lower
            .trim_matches(['(', ')'])
            .split_once(' ')
            .is_some_and(|(n, rest)| n.parse::<usize>().is_ok() && matches!(rest, "ord" | "words"))
        {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_budgets_share_the_allowance_and_exclude_metadata_and_notes() {
        let (messages, source) = fixture();
        let contract = CampaignContract::from_messages(&messages, &source).unwrap();
        for newline in ["\n", "\r\n"] {
            let candidate = "## LinkedIn 7. oktober\n\n**Vinkel:** En intern beskrivelse.\n\nDere kan lese.\n\nFire ord står her.\n\n### Kilder\n\nLang intern merknad teller ikke.\n\n## E-post\n\n**Emne:** Et langt emne\n\n**Brødtekst:**\n\nTakk for interessen.\n\nDere kan be om arket nå.\n\n### Kilder\n\nMer kildeinformasjon.".replace('\n', newline);
            let parts: Vec<_> = candidate.split(&format!("{newline}{newline}")).collect();
            let index = |text: &str| parts.iter().position(|part| *part == text).unwrap();
            let post = index("Dere kan lese.");
            let other = index("Fire ord står her.");
            let email = index("Takk for interessen.");
            let budgets = contract.repair_budgets(&candidate, &parts, &[post, other, email]);
            assert_eq!(budgets.len(), 2);
            assert_eq!(budgets[0].indexes, [post, other]);
            assert_eq!(budgets[0].current_body_words, 7);
            assert_eq!(budgets[0].unchanged_body_words, 0);
            assert_eq!(budgets[0].minimum_replacement_body_words, 5);
            assert_eq!(budgets[0].maximum_replacement_body_words, 9);
            assert_eq!(budgets[0].target_replacement_body_words, 7);
            assert_eq!(budgets[1].indexes, [email]);
            assert_eq!(budgets[1].current_body_words, 9);
            assert_eq!(budgets[1].unchanged_body_words, 6);
            assert_eq!(budgets[1].minimum_replacement_body_words, 2);
            assert_eq!(budgets[1].maximum_replacement_body_words, 8);
            assert_eq!(budgets[1].target_replacement_body_words, 5);
            let one = contract.repair_budgets(&candidate, &parts, &[post]);
            assert_eq!(one[0].unchanged_body_words, 4);
            assert_eq!(one[0].minimum_replacement_body_words, 1);
            assert_eq!(one[0].maximum_replacement_body_words, 5);
            assert!(contract.repair_budgets(&candidate, &parts, &[]).is_empty());
        }
    }

    #[test]
    fn whole_section_repair_budget_uses_only_body_words() {
        let (messages, source) = fixture();
        let contract = CampaignContract::from_messages(&messages, &source).unwrap();
        let candidate = "## E-post\n**Emne:** Long subject\n**Brødtekst:**\nFor kort.\n### Kilder\nLong source note that is not customer copy.";
        let parts: Vec<_> = copy_sections(candidate).into_iter().map(|(_, content)| content.trim()).collect();
        let budgets = contract.repair_budgets(candidate, &parts, &[0]);
        assert_eq!(budgets[0].current_body_words, 2);
        assert_eq!(budgets[0].unchanged_body_words, 0);
        assert_eq!(budgets[0].minimum_replacement_body_words, 8);
        assert_eq!(budgets[0].maximum_replacement_body_words, 14);
    }

    fn fixture() -> (Vec<ChatMessage>, SourceContext) {
        (vec![ChatMessage { role: "user".into(), content: "Bruk kampanjebriefen og språkprofilen til LinkedIn og e-post.".into(), ..Default::default() }],
        SourceContext { sources: vec![crate::source_validation::Source { id: 0, name: "brief".into(), content: "LinkedIn: to innlegg, 5–9 ord hver.\nE-post: brødtekst på 8–14 ord.\nSnakk til «dere».\nIngen emojier. Maks to emneknagger per LinkedIn-innlegg.\nIkke oppgi nettadresse.\nUnngå «garantert».".into() }] })
    }
    #[test]
    fn checks_exact_body_and_each_piece_without_metadata_or_notes() {
        let (messages, source) = fixture();
        let contract = CampaignContract::from_messages(&messages, &source).unwrap();
        let candidate = "# Draft\n\n## LinkedIn 7. oktober\n**Vinkel:** En ekstra lang intern beskrivelse.\nDere kan be om produktarket hos oss.\n\n### Kilder\nThis internal note may quote du.\n\n## LinkedIn 9. oktober\nHer finner dere informasjon om bordlampen vår.\n\n## E-post\n**Emne:** Et langt emne som ikke teller i brødtekst\n**Brødtekst:**\nTakk for interessen. Dere kan be oss om produktarket nå.\n### Kilder\nInternal source.\n";
        let checks = contract.validate(candidate).unwrap();
        let titled_checks = contract
            .validate(&candidate.replace("# Draft", "# LinkedIn og e-post: utkast"))
            .unwrap();
        assert_eq!(titled_checks.len(), checks.len());
        assert_eq!(
            checks.iter().map(|check| check.words).collect::<Vec<_>>(),
            [7, 7, 10]
        );
        let windows_copy = candidate
            .replace("### Kilder", "### Kildegrunnlag")
            .replace('\n', "\r\n");
        let windows_checks = contract.validate(&windows_copy).unwrap();
        assert_eq!(
            windows_checks
                .iter()
                .map(|check| (&check.body_hash, check.words))
                .collect::<Vec<_>>(),
            checks
                .iter()
                .map(|check| (&check.body_hash, check.words))
                .collect::<Vec<_>>()
        );
        assert!(contract
            .validate(&candidate.replace("Dere kan be", "Du kan be"))
            .unwrap_err()
            .contains("plural"));
        assert!(contract
            .validate(&candidate.replace("bordlampen vår", "garantert https://example.test 🌻"))
            .is_err());
        assert!(contract
            .validate(&candidate.replace("## LinkedIn 9. oktober", "## Annet"))
            .is_err());
    }
    #[test]
    fn file_cannot_adopt_itself_or_override_a_new_user_instruction() {
        let (mut messages, source) = fixture();
        messages[0].content = "Summarize the attachment.".into();
        assert!(CampaignContract::from_messages(&messages, &source).is_none());
        let (mut messages, source) = fixture();
        messages.push(ChatMessage {
            role: "user".into(),
            content: "Gjør innlegget til 30–40 ord.".into(),
            ..Default::default()
        });
        assert!(CampaignContract::from_messages(&messages, &source).is_none());
    }

    #[test]
    fn body_failures_bind_only_the_affected_section_and_keep_heading_gaps() {
        let (messages, source) = fixture();
        let contract = CampaignContract::from_messages(&messages, &source).unwrap();
        let candidate = "# Campaign\r\n\r\n## LinkedIn 7. oktober\r\nFor kort.\r\n\r\n## LinkedIn 9. oktober\r\nHer finner dere informasjon om bordlampen vår.\r\n\r\n## E-post\r\nBrødtekst:\r\nTakk for interessen. Dere kan be oss om produktarket nå.\r\n";
        let failed = contract.validate_scoped(candidate).unwrap_err();
        assert_eq!(failed.sections.len(), 1);
        assert_eq!(&candidate[failed.sections[0].range.clone()], "For kort.");
        let mut repaired = candidate.to_owned();
        repaired.replace_range(
            failed.sections[0].range.clone(),
            "Dere kan be om produktarket hos oss.",
        );
        assert!(contract.validate(&repaired).is_ok());
        assert!(repaired.ends_with(candidate.split_once("## LinkedIn 9.").unwrap().1));
        assert!(repaired.contains("hos oss.\r\n\r\n## LinkedIn 9."));
        assert!(contract
            .validate_scoped(&candidate.replace("## E-post", "## Other"))
            .unwrap_err()
            .sections
            .is_empty());
        assert!(contract
            .validate_scoped(&format!("{candidate}\n(42 ord)\n"))
            .unwrap_err()
            .sections
            .is_empty());
    }

    #[test]
    fn email_body_heading_excludes_recipient_and_subject_metadata_from_minimum() {
        let (messages, source) = fixture();
        let contract = CampaignContract::from_messages(&messages, &source).unwrap();
        for label in [
            "### Brødtekst:",
            "### **Brødtekst:**",
            "### Body:",
            "### E-posttekst:",
        ] {
            let candidate = format!("## LinkedIn 7. oktober\nDere kan be om produktarket hos oss.\n\n## LinkedIn 9. oktober\nHer finner dere informasjon om bordlampen vår.\n\n## E-post\n**Til:** Kontoransvarlige og små bedrifter som skal innrede kontorer\n**Emne:** Et arbeidslys\n\n{label}\n\nHei og takk.\n\n### Kilder\nLang intern merknad som heller ikke skal telle med.");
            let rejected = contract.validate_scoped(&candidate).unwrap_err();
            assert!(
                rejected.message.contains("body has 3 words, needs 8–14"),
                "{}",
                rejected.message
            );
            assert_eq!(rejected.sections.len(), 1);
            let corrected = candidate.replace(
                "Hei og takk.",
                "Takk for interessen. Dere kan be oss om produktarket nå.",
            );
            assert_eq!(contract.validate(&corrected).unwrap()[2].words, 10);
        }
    }
}
