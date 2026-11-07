//! Per-turn formatting guidance: short system notices that go into the prompt
//! only on the turns that need them.
//!
//! A notice here is not a review lever and not a standing rule. It is added
//! when the latest user message asks for the thing it explains, and on every
//! other turn the prompt is exactly what it was before this module existed:
//! [`for_turn`] returns `None`, the [`PromptContext`] slot stays empty, and
//! both [`PromptOrder`] variants render the same messages as before (see the
//! tests in `prompt_context.rs` and below).
//!
//! The first notice is the chart fence. The web app draws a closed
//! ```` ```chart ```` block natively (`ChatChart.tsx` in the Frontend Plane), so
//! on a turn that asks for a chart the model is told the JSON it may write.
//! Later notices (staged files, a deck path) add their own function and one
//! line to [`for_turn`], and share the same prompt slot.
//!
//! The widget notice (EP-4 S2) asks for a ```` ```widget ```` fence, which the
//! web app draws in a sandboxed frame. It is added only on a turn that asks for
//! something interactive, and only when the client can draw one (the
//! `inline_widgets` feature) and the gateway switch `MODEL_GATEWAY_WIDGET_GUIDANCE`
//! is on. Every other client keeps the prompt it had.
//!
//! [`PromptContext`]: crate::prompt_context::PromptContext
//! [`PromptOrder`]: crate::prompt_context::PromptOrder

use mp_contracts::model_plane::v1::ChatMessage;
use std::sync::OnceLock;

/// Only the start of a message is read: a request to draw a chart is a
/// sentence, and a pasted document must not cost a scan of every word.
const MAX_SCAN_CHARS: usize = 20_000;

/// How many words after a request verb a chart noun may follow it:
/// "lag en tabell ... og vis den som søylediagram" is a request, a verb in one
/// paragraph and a noun in another is not.
const MAX_WORDS_BETWEEN: usize = 9;

/// Words that ask for something to be made or shown, in Norwegian and English.
const REQUEST_VERBS: &[&str] = &[
    "lag", "lage", "lager", "vis", "vise", "viser", "tegn", "tegne", "tegner", "plott", "plotte",
    "plotter", "generer", "generere", "visualiser", "visualisere", "gjør", "konverter", "bygg",
    "make", "show", "draw", "plot", "create", "generate", "visualise", "visualize", "build",
    "convert", "turn", "chart", "graph",
];

/// Chart kinds that form a Norwegian compound with "diagram" ("søylediagram").
/// A flow, sequence or class diagram is a compound with another word and is
/// deliberately not here: those stay mermaid diagrams.
const CHART_KIND_PREFIXES: &[&str] = &[
    "søyle",
    "stolpe",
    "linje",
    "kake",
    "sektor",
    "sirkel",
    "areal",
    "område",
    "kurve",
    "punkt",
    "boble",
    "kolonne",
    "spredning",
    "ring",
];

/// Words that, directly before "chart", "graph" or "diagram", make it a
/// structural diagram or a data structure rather than a chart of numbers.
const STRUCTURAL_WORDS: &[&str] = &[
    "flow", "org", "organisation", "organization", "organizational", "gantt", "sequence", "class",
    "state", "network", "tree", "mind", "er", "uml", "swimlane", "process", "pert", "family",
    "venn", "knowledge", "social", "dependency", "call", "architecture", "system", "flyt",
    "sekvens", "klasse", "arkitektur",
];

/// Endings a Norwegian or English noun may take: "diagrammet", "diagrammer",
/// "diagrams", "grafen", "graphs".
const NOUN_ENDINGS: &[&str] = &["", "et", "met", "mer", "mene", "s", "es", "er", "ene", "en"];

/// The notice added on a turn that asks for a chart. It is the whole contract
/// between the model and the chart renderer: change one and change the other
/// (`parseChartFence` in the Frontend Plane's `lib/chat-chart/spec.ts`).
pub const CHART_NOTICE: &str = concat!(
    "The user is asking for a chart. Write it as a fenced code block whose language is `chart`, ",
    "containing exactly one JSON object; the app draws it as a native chart.\n",
    "Schema: {\"type\":\"bar\"|\"line\"|\"area\"|\"pie\",\"title\":\"short title\",",
    "\"x\":[\"category\",\"category\"],\"series\":[{\"name\":\"series name\",\"values\":[1,2]}],",
    "\"unit\":\"optional, e.g. MNOK or %\",\"stacked\":false}\n",
    "Rules: every value is a plain JSON number (no unit, no thousands separator), one per category ",
    "in every series. At most 24 categories and 6 series; a pie has one series and at most 8 slices. ",
    "Use line or area for values over time, bar to compare categories, pie only for the shares of one ",
    "whole. Use real numbers from the conversation or the sources; if the user asked for illustrative ",
    "numbers, say they are illustrative. Write the title, categories and series names in the user's ",
    "language. Close the fence.\n",
    "Add a sentence or two on what the chart shows, and do not repeat the data as a table unless the ",
    "user asks for one. Flow charts, org charts and other structural diagrams are not data charts: ",
    "keep using a mermaid block for those.",
);

/// Words that only look like a chart noun plus an ending ("grafer" needs "er"):
/// "charter" is not "chart" plus "er", nor "grapher" "graph" plus "er".
const NOT_CHART_NOUNS: &[&str] = &["charter", "grapher"];

/// Whether `word` (lowercase) names a chart. `previous` is the word before it.
fn is_chart_noun(word: &str, previous: Option<&str>) -> bool {
    if NOT_CHART_NOUNS.contains(&word) {
        return false;
    }
    let structural_before = previous.is_some_and(|before| STRUCTURAL_WORDS.contains(&before));
    for ending in NOUN_ENDINGS {
        let Some(stem) = word.strip_suffix(ending) else {
            continue;
        };
        match stem {
            "graf" | "histogram" => return true,
            "chart" | "graph" | "diagram" => return !structural_before,
            _ => {}
        }
        if let Some(kind) = stem.strip_suffix("diagram") {
            if CHART_KIND_PREFIXES.contains(&kind) {
                return true;
            }
        }
    }
    false
}

/// Whether the message asks for something `is_noun` names: a request verb followed, within a few
/// words, by such a noun. `is_noun` gets the word and the word before it, which a noun can need.
fn asks_for(user_text: &str, is_noun: impl Fn(&str, Option<&str>) -> bool) -> bool {
    let lowered: String = user_text
        .chars()
        .take(MAX_SCAN_CHARS)
        .flat_map(char::to_lowercase)
        .collect();
    let words: Vec<&str> = lowered
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.iter().enumerate().any(|(at, word)| {
        REQUEST_VERBS.contains(word)
            && (at + 1..words.len().min(at + 1 + MAX_WORDS_BETWEEN)).any(|candidate| {
                words.get(candidate).is_some_and(|&noun| {
                    is_noun(noun, candidate.checked_sub(1).and_then(|p| words.get(p)).copied())
                })
            })
    })
}

/// Whether the message asks for a chart: a request verb followed, within a
/// few words, by a chart noun ("lag et søylediagram", "plot this as a line
/// chart", "vis den som kakediagram"). A message that only mentions a chart
/// ("the chart is wrong"), asks for a flow diagram, or has no request does not.
fn asks_for_chart(user_text: &str) -> bool {
    asks_for(user_text, is_chart_noun)
}

/// The chart notice, only when the latest user message asks for a chart.
#[must_use]
pub fn chart_notice(user_text: &str) -> Option<&'static str> {
    asks_for_chart(user_text).then_some(CHART_NOTICE)
}

/// Words that name something a widget can be: a calculator, a slider, an interactive view, a
/// simulation. Matched anywhere in a word, so the Norwegian compounds ("lånekalkulator",
/// "skyveknappene") count.
const WIDGET_NOUN_STEMS: &[&str] = &[
    "kalkulator",
    "calculator",
    "interaktiv",
    "interactive",
    "widget",
    "simulering",
    "simulation",
    "skyveknapp",
    "slider",
];

/// Whether `word` (lowercase) names something a widget can be.
fn is_widget_noun(word: &str) -> bool {
    WIDGET_NOUN_STEMS.iter().any(|&stem| word.contains(stem))
}

/// Whether the message asks for something a widget can be: a request verb followed, within a few
/// words, by a widget noun ("lag en lånekalkulator", "vis en simulering av kølengden").
fn asks_for_widget(user_text: &str) -> bool {
    asks_for(user_text, |word, _previous| is_widget_noun(word))
}

/// The notice added on a turn that asks for something interactive, when the client can draw a
/// widget. It states the limits the frame enforces (`sandbox-html.ts` and `InlineWidget.tsx` in the
/// Frontend Plane): change one and change the other.
pub const WIDGET_NOTICE: &str = concat!(
    "The user is asking for something interactive. Use one only where the person has something to ",
    "do: change a value, pick an option, or run a calculation or a simulation. Write it as one fenced ",
    "code block whose language is `widget` (not `html`), holding one self-contained HTML document or ",
    "one SVG element. The app draws it inline in a sandbox with no network: no fetch, no external ",
    "scripts, stylesheets, fonts or images, no forms and no links. Inline script and CSS are allowed, ",
    "and so are inline SVG and data: URLs for images. Take colours and type from the host theme ",
    "variables --vv-background, --vv-surface, --vv-text, --vv-muted, --vv-accent, --vv-border, ",
    "--vv-radius, --vv-font-family and --vv-font-size, not fixed colours. Keep it under 100,000 ",
    "characters. In the answer, add a sentence or two on what the widget does and how to use it, and ",
    "do not repeat its code as prose. A chart or diagram the person only reads stays a `chart` or ",
    "`mermaid` block.",
);

/// The widget notice, only when the latest user message asks for something interactive.
#[must_use]
pub fn widget_notice(user_text: &str) -> Option<&'static str> {
    asks_for_widget(user_text).then_some(WIDGET_NOTICE)
}

/// Every notice that applies to this turn, joined; `None` when none does, which is every ordinary
/// turn. The widget notice needs `widgets_offered`: the client can draw one and the switch is on.
#[must_use]
pub fn for_turn(user_text: &str, widgets_offered: bool) -> Option<String> {
    let widget = if widgets_offered { widget_notice(user_text) } else { None };
    let notices: Vec<&str> = [chart_notice(user_text), widget].into_iter().flatten().collect();
    if notices.is_empty() {
        None
    } else {
        Some(notices.join("\n\n"))
    }
}

/// The system message for the prompt's `format_guidance` slot, for a client that may draw widgets.
#[must_use]
pub fn message_for_turn_with(user_text: &str, widgets_offered: bool) -> Option<ChatMessage> {
    for_turn(user_text, widgets_offered).map(|content| ChatMessage {
        role: "system".to_owned(),
        content,
        ..Default::default()
    })
}

/// The system message for a client that cannot draw a widget: the prompt is what it was before.
#[must_use]
pub fn message_for_turn(user_text: &str) -> Option<ChatMessage> {
    message_for_turn_with(user_text, false)
}

/// `MODEL_GATEWAY_WIDGET_GUIDANCE`: only the literal `true`, after trimming, turns it on.
fn widget_guidance_enabled_from(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether this gateway may write the widget notice (EP-4 S2). Read once per process, so it cannot
/// change under a request.
#[must_use]
pub fn widget_guidance_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        widget_guidance_enabled_from(
            std::env::var("MODEL_GATEWAY_WIDGET_GUIDANCE").ok().as_deref(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt_context::{PromptContext, PromptOrder};

    /// The four saved product journeys (`apps/verevon-web/plans/product-recordings/*/prompt.txt`),
    /// copied as literals: none of them may add anything to the prompt.
    const JOURNEY_PROMPTS: [&str; 4] = [
        "Lag et svarutkast til kunden om ordre FF-1042. Bruk den vedlagte henvendelsen, ordrestatusen og rutinene våre. Forklar hva vi vet, hva som må avklares, og hvilket neste steg du anbefaler. Svar på norsk, maks 150 ord. Vis kildehenvisninger i en egen intern merknad etter utkastet. Lever bare utkast; ikke send meldinger eller endre ordren.",
        "Lag en kort salgsrapport for uke 37 sammenlignet med uke 36, basert på vedlagt CSV og salgsnotat. Vis omsetning, bruttofortjeneste og vektet bruttomargin totalt og per produktgruppe. Finn de viktigste endringene, skill målte fakta fra mulige årsaker, og foreslå tre konkrete oppfølginger. Bruk norsk og vis beregningsgrunnlaget.",
        "Bruk kampanjebriefen, produktarket og språkprofilen til å lage tre LinkedIn-utkast for 21., 23. og 25. september og én kort e-post for Fjordform. Gi hvert innlegg en egen vinkel. Vis hvilken produktinformasjon tekstene bygger på. Lever alt som utkast til gjennomgang; ikke publiser, planlegg utsendelser eller send noe.",
        "Gjør møtenotatet om til en prosjektplan for kundeportalpiloten. Lag oppgaver med ansvarlig rolle, frist, avhengigheter og ferdigkriterium. Finn konflikter og risiko, skill vedtatte datoer fra dine forslag, og foreslå en realistisk rekkefølge frem mot 30. september. Lever planen som et utkast; ikke opprett kalenderavtaler eller send varsler.",
    ];

    #[test]
    fn the_saved_journey_prompts_get_no_chart_notice() {
        for prompt in JOURNEY_PROMPTS {
            assert_eq!(chart_notice(prompt), None, "{prompt}");
            assert_eq!(for_turn(prompt, false), None, "{prompt}");
            assert_eq!(for_turn(prompt, true), None, "{prompt}");
            assert_eq!(message_for_turn(prompt), None, "{prompt}");
        }
    }

    #[test]
    fn a_request_for_a_chart_gets_the_notice() {
        for prompt in [
            "lag et søylediagram av disse tallene",
            "plot this as a line chart",
            "Lag en tabell med omsetning per kvartal for 2025 og 2026 (fiktive tall) og vis den som søylediagram",
            "Vis dette som et kakediagram",
            "Kan du tegne en graf over salget?",
            "Make a pie chart of the market shares",
            "Show the revenue as a bar chart",
            "Can you draw a graph of these numbers?",
            "Lag et linjediagram",
            "lag et diagram av tallene over",
            "Gjør tabellen om til et områdediagram",
            "visualiser dette som et sektordiagram",
            "Turn this into a chart",
            "Create a stacked bar chart for each region",
            "lag et søyle-diagram",
            "Vis resultatet som diagrammet under",
            "LAG ET SØYLEDIAGRAM",
        ] {
            assert_eq!(chart_notice(prompt), Some(CHART_NOTICE), "{prompt}");
        }
    }

    #[test]
    fn ordinary_turns_get_nothing() {
        for prompt in [
            "",
            "   ",
            "Hva er hovedstaden i Norge?",
            "Skriv et utkast til en e-post til kunden",
            "Lag en tabell med omsetning per kvartal",
            "Vis meg tabellen igjen",
            "Summarise the diagram in the attachment",
            "Søylediagrammet er pent",
            "The chart is wrong in the last slide",
            "Hvor ligger grafikken?",
            "Vis meg grafikken til kampanjen",
            "Lag en grafisk profil",
            "What does a bar chart show?",
            "Create a project charter for the pilot",
            "Draw up the team charter",
        ] {
            assert_eq!(chart_notice(prompt), None, "{prompt}");
        }
    }

    #[test]
    fn flow_charts_and_other_structural_diagrams_are_not_data_charts() {
        for prompt in [
            "Lag et flytdiagram over prosessen",
            "Tegn et sekvensdiagram for innloggingen",
            "Lag et klassediagram",
            "Draw a flowchart of the checkout",
            "Make an org chart of the team",
            "Create a flow chart for onboarding",
            "Show me the gantt chart",
            "Draw a sequence diagram of the login",
            "Lag et arkitekturdiagram",
            "Show the knowledge graph",
        ] {
            assert_eq!(chart_notice(prompt), None, "{prompt}");
        }
    }

    #[test]
    fn the_verb_and_the_noun_must_be_in_the_same_stretch_of_words() {
        let far = "Lag en rapport om salget i Norge og Sverige for hele første halvår med tall fra regnskapet. Diagram";
        assert_eq!(chart_notice(far), None);
        assert_eq!(chart_notice("Lag diagram"), Some(CHART_NOTICE));
        // A noun before the verb is not a request.
        assert_eq!(chart_notice("diagram lag"), None);
    }

    #[test]
    fn only_the_start_of_a_very_long_message_is_read() {
        let long = format!("{} lag et søylediagram", "ord ".repeat(MAX_SCAN_CHARS));
        assert_eq!(chart_notice(&long), None);
        let short = format!("lag et søylediagram. {}", "ord ".repeat(MAX_SCAN_CHARS));
        assert_eq!(chart_notice(&short), Some(CHART_NOTICE));
    }

    #[test]
    fn the_notice_states_the_whole_fence_contract() {
        for needle in [
            "`chart`",
            "\"type\":\"bar\"|\"line\"|\"area\"|\"pie\"",
            "\"x\":",
            "\"series\":[{\"name\"",
            "\"values\":[1,2]",
            "\"unit\"",
            "\"stacked\"",
            "24 categories and 6 series",
            "8 slices",
            "Close the fence",
            "do not repeat the data as a table unless the user asks",
            "mermaid",
        ] {
            assert!(CHART_NOTICE.contains(needle), "missing {needle:?}");
        }
        assert!(!CHART_NOTICE.contains('\r'));
        assert!(CHART_NOTICE.len() < 1800, "{} bytes", CHART_NOTICE.len());
    }

    #[test]
    fn the_message_is_one_system_message_with_the_notice() {
        let message = message_for_turn("lag et søylediagram").expect("a chart request");
        assert_eq!(message.role, "system");
        assert_eq!(message.content, CHART_NOTICE);
        assert!(message.name.is_empty());
    }

    fn user(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_owned(),
            content: content.to_owned(),
            ..Default::default()
        }
    }

    /// What the SSE path does with the slot, for one user message.
    fn rendered(user_text: &str, order: PromptOrder) -> Vec<ChatMessage> {
        PromptContext {
            format_guidance: message_for_turn(user_text),
            history: vec![user(user_text)],
            ..Default::default()
        }
        .render(order)
    }

    #[test]
    fn the_saved_journey_prompts_render_exactly_as_without_the_slot() {
        for prompt in JOURNEY_PROMPTS {
            for order in [PromptOrder::Legacy, PromptOrder::StablePrefix] {
                let without = PromptContext {
                    history: vec![user(prompt)],
                    ..Default::default()
                }
                .render(order);
                assert_eq!(rendered(prompt, order), without, "{prompt}");
                assert_eq!(without, vec![user(prompt)]);
            }
        }
    }

    #[test]
    fn a_chart_request_adds_exactly_one_system_message_ahead_of_the_user_turn() {
        let prompt = "plot this as a line chart";
        for order in [PromptOrder::Legacy, PromptOrder::StablePrefix] {
            let out = rendered(prompt, order);
            assert_eq!(out.len(), 2, "{order:?}");
            assert_eq!(out[0].role, "system");
            assert_eq!(out[0].content, CHART_NOTICE);
            assert_eq!(out[1], user(prompt));
        }
    }

    #[test]
    fn widget_notice_needs_a_request() {
        for prompt in [
            "Lag en lånekalkulator med skyveknapp for rente",
            "Make an interactive calculator for mortgage payments",
            "Show a simulation of the queue",
            "Vis en simulering av kølengden",
            "Build a calculator widget",
            "Lag et interaktivt diagram over sparingen",
        ] {
            assert_eq!(widget_notice(prompt), Some(WIDGET_NOTICE), "{prompt}");
        }
        for prompt in [
            "Kalkulatoren gir feil svar",
            "The widget is broken",
            "Hva er en interaktiv tavle?",
            "Vis meg tabellen igjen",
            "Lag en tabell med omsetning",
        ] {
            assert_eq!(widget_notice(prompt), None, "{prompt}");
        }
    }

    #[test]
    fn widget_notice_off_without_the_feature() {
        // A client that cannot draw a widget gets no notice for a widget request.
        let prompt = "Lag en lånekalkulator med skyveknapp for rente";
        assert_eq!(for_turn(prompt, false), None);
        assert_eq!(message_for_turn_with(prompt, false), None);
        assert_eq!(message_for_turn(prompt), None);
        assert_eq!(for_turn(prompt, true), Some(WIDGET_NOTICE.to_owned()));
    }

    #[test]
    fn chart_notice_unchanged() {
        // A chart request gets the chart notice alone, whether or not widgets are offered.
        assert_eq!(for_turn("lag et søylediagram", false), Some(CHART_NOTICE.to_owned()));
        assert_eq!(for_turn("lag et søylediagram", true), Some(CHART_NOTICE.to_owned()));
        // A request for an interactive chart gets both notices, the chart first.
        assert_eq!(
            for_turn("lag et interaktivt søylediagram", true),
            Some(format!("{CHART_NOTICE}\n\n{WIDGET_NOTICE}")),
        );
    }

    #[test]
    fn message_for_turn_is_unchanged_when_widgets_not_offered() {
        let prompts = JOURNEY_PROMPTS.into_iter().chain([
            "lag et søylediagram av disse tallene",
            "Lag en lånekalkulator med skyveknapp for rente",
        ]);
        for prompt in prompts {
            assert_eq!(message_for_turn(prompt), message_for_turn_with(prompt, false), "{prompt}");
        }
    }

    #[test]
    fn the_switch_is_the_literal_true() {
        assert!(!widget_guidance_enabled_from(None));
        assert!(!widget_guidance_enabled_from(Some("")));
        assert!(!widget_guidance_enabled_from(Some("1")));
        assert!(!widget_guidance_enabled_from(Some("TRUE")));
        assert!(widget_guidance_enabled_from(Some("true")));
        assert!(widget_guidance_enabled_from(Some(" true ")));
    }

    #[test]
    fn the_widget_notice_states_the_frame_limits() {
        for needle in [
            "`widget`",
            "(not `html`)",
            "one fenced code block",
            "no network",
            "no external scripts",
            "data: URLs",
            "--vv-text",
            "--vv-accent",
            "100,000 characters",
            "`chart` or `mermaid` block",
        ] {
            assert!(WIDGET_NOTICE.contains(needle), "missing {needle:?}");
        }
        assert!(!WIDGET_NOTICE.contains('\r'));
        assert!(WIDGET_NOTICE.len() < 1600, "{} bytes", WIDGET_NOTICE.len());
    }
}
