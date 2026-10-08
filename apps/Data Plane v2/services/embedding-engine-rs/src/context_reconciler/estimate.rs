//! What a run would cost, from the counts alone: the dry run's report, and the figure
//! `--max-usd` is held to.
//!
//! Nothing in here reads a document. The counts are of units and of characters (see
//! `select::counts`), and the estimate is arithmetic on them, so a dry run can be shown, logged
//! and pasted into a ticket: it contains numbers and organization ids and no content.
//!
//! ## What the figures are
//!
//! * A generation sends the document's window (capped at the configured size) and the chunk,
//!   with the instructions around them, and the answer is at most `max_tokens`. Tokens are
//!   estimated at four characters each, the same estimate the cost event has always used, and
//!   the answer is counted at its cap, so the estimate is an upper bound and never a
//!   surprise in the other direction.
//! * The prices are the Model Plane's own catalogue row for the model
//!   (`cost-core/internal/pricing`: gpt-4o-mini at 0.15 and 0.60 USD per million tokens, input
//!   and output), overridable. The embedding price is NOT defaulted: no price for the embedding
//!   model is in the repository, so unless one is given the report counts the texts and the
//!   tokens and says the cost is unknown (`null`), and `--max-usd` bounds the generation spend
//!   only, which the report says too.

use anyhow::Context as _;
use serde_json::{json, Value};

use super::select::OrgCounts;

/// Characters per token, for every estimate here.
const CHARS_PER_TOKEN: f64 = 4.0;

/// The instructions around the document and the chunk, in tokens (the ctx-v1 and ctx-v2 prompts
/// are both a few hundred characters).
const PROMPT_OVERHEAD_TOKENS: i64 = 250;

/// What a stored context adds to the text that is embedded, in characters (the validator keeps
/// 12 to 600; a context is one or two sentences).
const CONTEXT_CHARS: i64 = 300;

/// The prices an estimate is made with, in USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Prices {
    pub llm_input: f64,
    pub llm_output: f64,
    /// `None`: not known, and the estimate says so.
    pub embed: Option<f64>,
}

impl Default for Prices {
    fn default() -> Self {
        Self {
            llm_input: 0.15,
            llm_output: 0.60,
            embed: None,
        }
    }
}

impl Prices {
    /// The defaults, overridden by `CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK`,
    /// `CONTEXT_ESTIMATE_OUTPUT_USD_PER_MTOK` and `CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK`.
    pub(crate) fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let number = |name: &str| -> anyhow::Result<Option<f64>> {
            match lookup(name).filter(|value| !value.trim().is_empty()) {
                None => Ok(None),
                Some(value) => {
                    let parsed: f64 = value
                        .trim()
                        .parse()
                        .with_context(|| format!("{name} must be a number"))?;
                    if !parsed.is_finite() || parsed < 0.0 {
                        anyhow::bail!("{name} must be zero or more, got {value}");
                    }
                    Ok(Some(parsed))
                }
            }
        };
        let defaults = Self::default();
        Ok(Self {
            llm_input: number("CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK")?.unwrap_or(defaults.llm_input),
            llm_output: number("CONTEXT_ESTIMATE_OUTPUT_USD_PER_MTOK")?
                .unwrap_or(defaults.llm_output),
            embed: number("CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK")?,
        })
    }
}

/// What the work of one organization would use.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Estimate {
    /// Generations: one provider call per unit (a retry is another, which an estimate cannot
    /// know).
    pub llm_calls: i64,
    pub llm_input_tokens: i64,
    /// At the cap of the answer.
    pub llm_output_tokens: i64,
    pub llm_usd: f64,
    /// Texts to embed: the units that are generated, the legacy ones, the incoherent ones.
    pub embed_texts: i64,
    pub embed_tokens: i64,
    /// `None` when no embedding price was given.
    pub embed_usd: Option<f64>,
}

fn tokens(chars: i64) -> i64 {
    (chars.max(0) as f64 / CHARS_PER_TOKEN).ceil() as i64
}

/// The estimate for one organization. `with_refresh` adds the units a prompt refresh would
/// regenerate; `max_output_tokens` is `CONTEXTUAL_RETRIEVAL_MAX_TOKENS`.
pub(crate) fn estimate(
    counts: &OrgCounts,
    with_refresh: bool,
    prices: &Prices,
    max_output_tokens: i64,
) -> Estimate {
    let (refresh, refresh_chars, refresh_text_chars) = if with_refresh {
        (
            counts.refresh,
            counts.refresh_chars,
            counts.refresh_text_chars,
        )
    } else {
        (0, 0, 0)
    };
    let llm_calls = counts.generate + refresh;
    let llm_input_tokens =
        tokens(counts.generate_chars + refresh_chars) + llm_calls * PROMPT_OVERHEAD_TOKENS;
    let llm_output_tokens = llm_calls * max_output_tokens.max(0);
    let per_mtok = |count: i64, price: f64| count as f64 / 1_000_000.0 * price;
    let llm_usd = per_mtok(llm_input_tokens, prices.llm_input)
        + per_mtok(llm_output_tokens, prices.llm_output);

    let embed_texts = counts.generate + refresh + counts.verify + counts.repair;
    let embed_tokens = tokens(counts.generate_text_chars + refresh_text_chars + counts.embed_chars)
        + tokens((counts.generate + refresh) * CONTEXT_CHARS);
    Estimate {
        llm_calls,
        llm_input_tokens,
        llm_output_tokens,
        llm_usd,
        embed_texts,
        embed_tokens,
        embed_usd: prices.embed.map(|price| per_mtok(embed_tokens, price)),
    }
}

impl Estimate {
    pub(crate) fn plus(&self, other: &Estimate) -> Estimate {
        Estimate {
            llm_calls: self.llm_calls + other.llm_calls,
            llm_input_tokens: self.llm_input_tokens + other.llm_input_tokens,
            llm_output_tokens: self.llm_output_tokens + other.llm_output_tokens,
            llm_usd: self.llm_usd + other.llm_usd,
            embed_texts: self.embed_texts + other.embed_texts,
            embed_tokens: self.embed_tokens + other.embed_tokens,
            embed_usd: match (self.embed_usd, other.embed_usd) {
                (Some(a), Some(b)) => Some(a + b),
                (None, None) => None,
                (Some(a), None) | (None, Some(a)) => Some(a),
            },
        }
    }

    fn to_json(self) -> Value {
        let usd = |value: f64| (value * 1_000_000.0).round() / 1_000_000.0;
        json!({
            "llm_calls": self.llm_calls,
            "llm_input_tokens": self.llm_input_tokens,
            "llm_output_tokens_at_cap": self.llm_output_tokens,
            "llm_usd": usd(self.llm_usd),
            "embed_texts": self.embed_texts,
            "embed_tokens": self.embed_tokens,
            "embed_usd": self.embed_usd.map(usd),
        })
    }
}

/// One JSON object per line: an organization each, then the total. Counts, ids and estimates,
/// and nothing a document or a context says.
pub(crate) fn render(
    counts: &[OrgCounts],
    prices: &Prices,
    max_output_tokens: i64,
    refresh_target: Option<&str>,
) -> String {
    let with_refresh = refresh_target.is_some();
    let mut lines = Vec::new();
    let mut total = Estimate::default();
    let mut sum = OrgCounts::default();
    for org in counts {
        let estimated = estimate(org, with_refresh, prices, max_output_tokens);
        total = total.plus(&estimated);
        sum.generate += org.generate;
        sum.skipped_policy += org.skipped_policy;
        sum.skipped_budget += org.skipped_budget;
        sum.verify += org.verify;
        sum.repair += org.repair;
        sum.refresh += org.refresh;
        sum.exhausted += org.exhausted;
        sum.skipped_zdr += org.skipped_zdr;
        sum.waiting += org.waiting;
        lines.push(unit_counts(&org.org_id, org, estimated, refresh_target));
    }
    lines.push(unit_counts("*", &sum, total, refresh_target));
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn unit_counts(org: &str, counts: &OrgCounts, estimate: Estimate, target: Option<&str>) -> Value {
    json!({
        "org_id": org,
        "units": {
            "generate": counts.generate,
            "skipped_policy": counts.skipped_policy,
            "skipped_budget": counts.skipped_budget,
            "verify_legacy": counts.verify,
            "repair_vector": counts.repair,
            "refresh": target.map(|_| counts.refresh),
            "failed_exhausted": counts.exhausted,
            "skipped_zdr": counts.skipped_zdr,
            "waiting": counts.waiting,
        },
        "refresh_target": target,
        "estimate": estimate.to_json(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts() -> OrgCounts {
        OrgCounts {
            org_id: "org-a".into(),
            generate: 10,
            generate_chars: 100_000,
            generate_text_chars: 4_000,
            skipped_policy: 2,
            skipped_budget: 4,
            verify: 206,
            repair: 4,
            embed_chars: 84_000,
            refresh: 5,
            refresh_chars: 50_000,
            refresh_text_chars: 2_000,
            exhausted: 1,
            skipped_zdr: 3,
            waiting: 7,
        }
    }

    #[test]
    fn generation_is_priced_by_the_catalogues_row_and_the_answer_is_counted_at_its_cap() {
        let estimated = estimate(&counts(), false, &Prices::default(), 128);
        assert_eq!(estimated.llm_calls, 10);
        // 100,000 characters at four a token, and the instructions around each of ten prompts.
        assert_eq!(estimated.llm_input_tokens, 25_000 + 10 * 250);
        assert_eq!(estimated.llm_output_tokens, 1_280);
        let expected = 27_500.0 / 1e6 * 0.15 + 1_280.0 / 1e6 * 0.60;
        assert!(
            (estimated.llm_usd - expected).abs() < 1e-12,
            "{}",
            estimated.llm_usd
        );
        // Texts to embed: the ten generated, the 206 legacy, the four to repair.
        assert_eq!(estimated.embed_texts, 220);
        assert_eq!(estimated.embed_tokens, 1_000 + 21_000 + 750);
        assert_eq!(estimated.embed_usd, None, "no embedding price is made up");
    }

    #[test]
    fn a_refresh_adds_its_units_only_when_it_is_asked_for() {
        let without = estimate(&counts(), false, &Prices::default(), 128);
        let with = estimate(&counts(), true, &Prices::default(), 128);
        assert_eq!(with.llm_calls - without.llm_calls, 5);
        assert_eq!(with.embed_texts - without.embed_texts, 5);
        assert!(with.llm_usd > without.llm_usd);
    }

    #[test]
    fn an_embedding_price_is_used_when_one_is_given_and_never_guessed() {
        let prices = Prices {
            embed: Some(0.12),
            ..Prices::default()
        };
        let estimated = estimate(&counts(), false, &prices, 128);
        let expected = estimated.embed_tokens as f64 / 1e6 * 0.12;
        assert!((estimated.embed_usd.unwrap() - expected).abs() < 1e-12);
    }

    #[test]
    fn prices_come_from_the_environment_and_a_bad_one_is_refused() {
        let prices = Prices::from_lookup(|name| match name {
            "CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK" => Some("0.30".into()),
            "CONTEXT_ESTIMATE_EMBED_USD_PER_MTOK" => Some(" 0.12 ".into()),
            "CONTEXT_ESTIMATE_OUTPUT_USD_PER_MTOK" => Some(String::new()),
            _ => None,
        })
        .expect("prices");
        assert_eq!(
            prices,
            Prices {
                llm_input: 0.30,
                llm_output: 0.60,
                embed: Some(0.12)
            }
        );
        for bad in ["abc", "-1", "NaN", "inf"] {
            let error = Prices::from_lookup(|name| {
                (name == "CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK").then(|| bad.to_owned())
            })
            .expect_err(bad)
            .to_string();
            assert!(
                error.contains("CONTEXT_ESTIMATE_INPUT_USD_PER_MTOK"),
                "{error}"
            );
        }
    }

    #[test]
    fn the_report_is_one_line_per_organization_and_a_total_and_holds_no_content() {
        let mut second = counts();
        second.org_id = "org-b".into();
        let report = render(&[counts(), second], &Prices::default(), 128, None);
        let lines: Vec<Value> = report
            .lines()
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["org_id"], "org-a");
        assert_eq!(lines[1]["org_id"], "org-b");
        assert_eq!(lines[2]["org_id"], "*");
        assert_eq!(lines[2]["units"]["verify_legacy"], 412);
        assert_eq!(lines[2]["units"]["generate"], 20);
        // The units a used-up budget holds are counted apart from the work (CR-T9b), per
        // organization and in the total.
        assert_eq!(lines[0]["units"]["skipped_budget"], 4);
        assert_eq!(lines[2]["units"]["skipped_budget"], 8);
        assert_eq!(
            lines[0]["units"]["refresh"],
            Value::Null,
            "no refresh was asked for"
        );
        assert_eq!(lines[2]["estimate"]["llm_calls"], 20);
        assert_eq!(lines[2]["estimate"]["embed_usd"], Value::Null);
        // The only strings in it are the keys, the organization ids and the target.
        fn strings(value: &Value, out: &mut Vec<String>) {
            match value {
                Value::String(text) => out.push(text.clone()),
                Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
                Value::Object(map) => map.values().for_each(|item| strings(item, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        lines.iter().for_each(|line| strings(line, &mut found));
        found.sort();
        found.dedup();
        assert_eq!(found, ["*", "org-a", "org-b"]);

        let with_target = render(&[counts()], &Prices::default(), 128, Some("ctx-v2"));
        let first: Value = serde_json::from_str(with_target.lines().next().unwrap()).unwrap();
        assert_eq!(first["refresh_target"], "ctx-v2");
        assert_eq!(first["units"]["refresh"], 5);
    }
}
