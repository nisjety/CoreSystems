//! Source relevance: can this search hit plausibly answer *this* question?
//!
//! ## Why this module exists
//!
//! Neither the inline `web_search` path nor [`crate::deep_research`] had a
//! relevance gate. Every hit a search provider returned became a candidate, and
//! once a candidate was emitted it became a citation. Live, a Norwegian weather
//! question produced sixteen "sources" including an Instagram post about Paris
//! cafés, an FHI paper on skeletal age assessment, a `TikTok` perfume video and a
//! `LinkedIn` profile — all shown in the Kilder tab as if they were evidence.
//! `deep_research::read_order` ranked by corroboration, which made it *worse*:
//! six sub-queries all surfacing the same off-topic page counted as six
//! independent votes for noise.
//!
//! ## What it does and does not claim
//!
//! This is a **plausibility filter, not a ranker**. It answers "could this page
//! be about the question at all", using signals that can be justified line by
//! line:
//!
//! 1. **Quarry's own reranker score**, when present. Quarry's search stack wraps
//!    its provider in a `RerankingSearchProvider` that asks the Model Plane to
//!    score how well each of the top-N results answers the query, and returns
//!    that as `score` in `[0,1]`. It is a real relevance judgement against the
//!    same query and therefore outranks anything we can compute here. It is
//!    absent (not zero) when reranking is disabled or the hit fell outside the
//!    reranked head — see [`Candidate::provider_score`].
//! 2. **Lexical overlap** between the question's content terms and the hit's
//!    title + snippet, stopword-stripped and lightly stemmed in Norwegian *and*
//!    English (the product's primary language is Norwegian; an English-only
//!    stopword list would leave `hva`, `er` and `i` in the term set and score
//!    every Norwegian page against noise).
//! 3. **A soft-demoted host class** for social / short-video / UGC / aggregator
//!    hosts, which for a factual question return someone's post rather than a
//!    source. This only ever **demotes** — never blocks — and is suppressed
//!    entirely when the question itself names the platform, because a question
//!    *about* `TikTok` legitimately wants `tiktok.com`.
//! 4. **An implied-domain bonus**: a Norwegian statistics question implies
//!    `ssb.no`, a weather question implies `met.no` / `yr.no`.
//!
//! Deliberately NOT here: an invented "ML score". Every number below is either
//! Quarry's, a term-overlap ratio, or a constant with a written justification.
//!
//! ## Honesty contract
//!
//! This module returns verdicts; it never deletes anything. Callers are required
//! to keep a dropped hit visible and counted — in `deep_research` a filtered
//! source keeps its Kilder row with a `[not read: …]` label, is listed in the
//! synthesis prompt as explicitly uncitable, is excluded from the read budget,
//! and can never receive a citation number. And the gate never empties a result
//! set: see [`keep_mask`].

use std::collections::BTreeSet;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Tunables. Same rule as `deep_research`'s caps: every one is env-overridable
// and clamped to a hard range, because a threshold that can be set to 1.0 is a
// switch that silently deletes every source.
// ---------------------------------------------------------------------------

/// Credit for a question term found only in the snippet, against 1.0 for a term
/// in the title.
///
/// A term in the title is what the page is *about*; a term in the snippet may be
/// navigation, a cookie banner or a "related articles" strip that the provider
/// happened to excerpt. 0.7 rather than something harsher because provider
/// snippets are usually query-focused, so snippet-only matches are mostly real.
const SNIPPET_ONLY_CREDIT: f32 = 0.7;

/// Weight on Quarry's reranker score when it supplied one; lexical overlap takes
/// the remainder.
///
/// 0.6 — the reranker read the query and the result together and is strictly
/// better evidence than bag-of-words overlap, so it dominates. It does not get
/// the whole vote because it is one LLM call that can be wrong, and because it
/// only ever scores the head of the list: leaving lexical a real 0.4 vote keeps
/// the two halves of a response scored on a comparable footing.
const DEFAULT_PROVIDER_WEIGHT: f32 = 0.6;
const MIN_PROVIDER_WEIGHT: f32 = 0.0;
const MAX_PROVIDER_WEIGHT: f32 = 1.0;

/// Penalty for a hit on a [soft-demoted host](DEFAULT_SOFT_HOSTS).
///
/// 0.35 is calibrated against [`DEFAULT_KEEP_THRESHOLD`]: it is large enough to
/// push a *weakly* matching social hit (the observed noise: 0.0–0.3 overlap)
/// below the keep line, and small enough that a hit which genuinely matches the
/// question survives on its own merits — a `tiktok.com` page with full term
/// coverage still scores 1.0 − 0.35 = 0.65, comfortably kept. That asymmetry is
/// the whole point: demote, never block.
const DEFAULT_SOFT_HOST_PENALTY: f32 = 0.35;
const MIN_SOFT_HOST_PENALTY: f32 = 0.0;
/// A penalty of 1.0 would be a hard block, which this module refuses to have.
const MAX_SOFT_HOST_PENALTY: f32 = 0.9;

/// Extra penalty for a personal-profile path (`/in/`, `/pub/`, `/profile/`).
///
/// A person's profile page is never the source of a fact about the world, even
/// on a host that also publishes real articles. Additive on top of the host
/// penalty, and still only a demotion — a question that names the person can
/// out-score it on term coverage.
const PROFILE_PATH_PENALTY: f32 = 0.15;

/// Bonus for a host the question's own topic implies.
///
/// 0.25 lifts an authoritative hit over an equally-worded generic one, which is
/// the ordering we want. It cannot *admit* an off-topic authority: 0.0 overlap
/// plus 0.25 is still below [`DEFAULT_KEEP_THRESHOLD`], which is exactly why the
/// live FHI-paper-for-a-weather-question case stays filtered.
const DEFAULT_IMPLIED_DOMAIN_BONUS: f32 = 0.25;
const MIN_IMPLIED_DOMAIN_BONUS: f32 = 0.0;
const MAX_IMPLIED_DOMAIN_BONUS: f32 = 0.5;

/// Score at or above which a hit may be read and cited.
///
/// 0.30 is derived, not picked: with [`SNIPPET_ONLY_CREDIT`] at 0.7 a hit clears
/// it by covering roughly **half** the question's content terms in its snippet
/// (0.5 × 0.7 = 0.35) or **a third** of them in its title (0.33). Below that the
/// hit shares fewer than half the words the question is actually made of, which
/// is where all four observed noise cases sat — they shared *none*. Raising it
/// costs recall on pages whose title is a headline rather than a restatement of
/// the question; lowering it re-admits the noise.
const DEFAULT_KEEP_THRESHOLD: f32 = 0.30;
const MIN_KEEP_THRESHOLD: f32 = 0.0;
/// Above ~0.8 only a page whose title nearly restates the question survives, so
/// the ceiling stops a misconfiguration from emptying every research turn.
const MAX_KEEP_THRESHOLD: f32 = 0.8;

/// How many top-scored hits survive when the gate would otherwise drop all of
/// them.
///
/// An empty result set is a worse failure than a noisy one: the user gets
/// nothing *and* no explanation. Three, because the caller states the fallback
/// in its coverage line, and three sources is enough for the model to notice
/// they disagree with each other.
const DEFAULT_FALLBACK_KEEP: usize = 3;
const MIN_FALLBACK_KEEP: usize = 1;
const MAX_FALLBACK_KEEP: usize = 10;

/// Neutral score for a question with no content terms at all ("hva er det?",
/// "why?").
///
/// We cannot judge relevance without terms to judge it against, so the gate
/// becomes a no-op rather than guessing. 1.0, not 0.0: a filter that cannot see
/// must not delete.
const UNJUDGEABLE_SCORE: f32 = 1.0;

/// Shortest stem we will produce. Below three characters a Norwegian stem stops
/// identifying a word ("er", "en") and starts matching everything.
const MIN_STEM_CHARS: usize = 3;

/// Shortest platform alias that may exempt its host class. `x.com`'s label is
/// one character, and treating a bare "x" in a question as "the user means
/// Twitter" would exempt the class on almost every question.
const MIN_PLATFORM_ALIAS_CHARS: usize = 3;

/// Hosts whose pages, for a factual question, are someone's post rather than a
/// source: social networks, short-video platforms, UGC forums used as a primary
/// source, and link aggregators.
///
/// This is a **relevance heuristic for factual questions, not a judgement about
/// the platforms.** Membership only ever subtracts
/// [`DEFAULT_SOFT_HOST_PENALTY`]; it never blocks, and it does not apply at all
/// when the question names the platform ([`Question::names_platform`]).
///
/// Override wholesale with `VEREVON_RELEVANCE_SOFT_HOSTS` as a comma-separated
/// host list (an empty value disables the class entirely). Matching is on the
/// registrable suffix, so `no.linkedin.com` and `www.linkedin.com` both match
/// `linkedin.com`.
#[rustfmt::skip]
const DEFAULT_SOFT_HOSTS: &[&str] = &[
    // Social networks / UGC feeds.
    "instagram.com",
    "facebook.com",
    "fb.com",
    "threads.net",
    "x.com",
    "twitter.com",
    "linkedin.com",
    "pinterest.com",
    "pinterest.no",
    "tumblr.com",
    "vk.com",
    "snapchat.com",
    "reddit.com",
    "quora.com",
    // Short-video / video-first platforms.
    "tiktok.com",
    "youtube.com",
    "youtu.be",
    "vimeo.com",
    "dailymotion.com",
    "twitch.tv",
    // Link aggregators and content-farm re-publishers: no primary content of
    // their own, so a hit here is a pointer at a source rather than a source.
    "flipboard.com",
    "scoop.it",
    "paper.li",
    "pearltrees.com",
    "slideshare.net",
];

/// Path prefixes that identify a personal profile rather than a document.
#[rustfmt::skip]
const PROFILE_PATH_MARKERS: &[&str] = &["/in/", "/pub/", "/profile/", "/user/", "/users/", "/@"];

/// Topic trigger → hosts that topic implies, as authoritative Norwegian and EU
/// primary sources.
///
/// Triggers are matched against the question's **stemmed** content terms, so
/// `statistikk` also fires on `statistikken` and `statistikker`. The table is a
/// documented constant rather than config because each row encodes a real
/// ownership fact ("population statistics for Norway live at SSB"), not a
/// preference — but it is additive only, so a missing row costs a bonus, never a
/// source.
const IMPLIED_DOMAINS: &[(&str, &[&str])] = &[
    // Weather — the exact live failure this module was written for.
    ("vær", &["met.no", "yr.no"]),
    ("været", &["met.no", "yr.no"]),
    ("værvarsel", &["met.no", "yr.no"]),
    ("temperatur", &["met.no", "yr.no"]),
    ("nedbør", &["met.no", "yr.no"]),
    ("weather", &["met.no", "yr.no"]),
    ("forecast", &["met.no", "yr.no"]),
    // Official statistics.
    ("statistikk", &["ssb.no"]),
    ("befolkning", &["ssb.no"]),
    ("folketall", &["ssb.no"]),
    ("konsumprisindeks", &["ssb.no"]),
    ("konsumprisindeksen", &["ssb.no"]),
    ("inflasjon", &["ssb.no", "norges-bank.no"]),
    ("kpi", &["ssb.no"]),
    ("arbeidsledighet", &["ssb.no", "nav.no"]),
    ("styringsrente", &["norges-bank.no"]),
    // Law and regulation.
    ("lov", &["lovdata.no", "regjeringen.no"]),
    ("forskrift", &["lovdata.no"]),
    ("paragraf", &["lovdata.no"]),
    ("lovdata", &["lovdata.no"]),
    // Company / entity registers.
    ("organisasjonsnummer", &["brreg.no"]),
    ("foretaksregisteret", &["brreg.no"]),
    ("brreg", &["brreg.no"]),
    ("regnskap", &["brreg.no", "proff.no"]),
    // Property and maps.
    ("matrikkel", &["kartverket.no"]),
    ("eiendom", &["kartverket.no"]),
    ("kartverket", &["kartverket.no"]),
    // Tax and benefits.
    ("skatt", &["skatteetaten.no"]),
    ("merverdiavgift", &["skatteetaten.no"]),
    ("mva", &["skatteetaten.no"]),
    ("dagpenger", &["nav.no"]),
    ("sykepenger", &["nav.no"]),
    ("altinn", &["altinn.no"]),
    // Health.
    ("folkehelse", &["fhi.no", "helsenorge.no"]),
    ("vaksine", &["fhi.no", "helsenorge.no"]),
    ("smittevern", &["fhi.no"]),
    ("helsenorge", &["helsenorge.no"]),
    // Energy — the offshore-wind research turn that produced the same noise.
    ("strømpris", &["nve.no", "statnett.no"]),
    ("kraftproduksjon", &["nve.no", "statnett.no"]),
    (
        "havvind",
        &["nve.no", "regjeringen.no", "energifaktanorge.no"],
    ),
    ("vindkraft", &["nve.no", "energifaktanorge.no"]),
    ("vannkraft", &["nve.no", "energifaktanorge.no"]),
    ("energi", &["nve.no", "energifaktanorge.no"]),
    ("petroleum", &["sodir.no", "norskpetroleum.no"]),
    ("sokkel", &["sodir.no", "norskpetroleum.no"]),
    // Transport.
    ("vegvesen", &["vegvesen.no"]),
    ("kjøretøy", &["vegvesen.no"]),
];

/// Norwegian stopwords, bokmål and nynorsk together.
///
/// The product's primary language is Norwegian, so this list — not the English
/// one — is what keeps scoring honest. Without it `hva er været i Oslo` carries
/// the content terms `hva`, `er` and `i`, every page on the web matches at least
/// one of them, and the overlap ratio stops discriminating.
///
/// Note `være` / `vere` / `vært` / `vore` (the verb "to be") are stopwords while
/// `vær` / `været` (the weather) are not; both sides of a comparison run through
/// the same pipeline, so the verb is removed from page text too and cannot be
/// mistaken for the noun.
#[rustfmt::skip]
const NORWEGIAN_STOPWORDS: &[&str] = &[
    "alle", "alt", "andre", "at", "av", "bare", "begge", "bli", "blir", "blitt", "blei", "både",
    "båe", "da", "de", "dei", "deim", "deira", "deires", "dem", "den", "denne", "der", "dere",
    "deres", "det", "dette", "di", "din", "disse", "ditt", "du", "dykk", "dykkar", "då", "eg",
    "ein", "eit", "eitt", "eller", "elles", "en", "enn", "er", "et", "ett", "etter", "for",
    "fordi", "fra", "før", "ha", "hadde", "han", "hans", "har", "hennar", "henne", "hennes", "her",
    "hjå", "ho", "hoe", "honom", "hoss", "hossen", "hun", "hva", "hvem", "hver", "hvilke",
    "hvilken", "hvis", "hvor", "hvordan", "hvorfor", "i", "ikke", "ikkje", "ingen", "ingi",
    "inkje", "inn", "inni", "ja", "jeg", "kan", "korleis", "korso", "kun", "kunne", "kva", "kvar",
    "kvarhelst", "kven", "kvi", "kvifor", "man", "mange", "med", "medan", "meg", "meget", "mellom",
    "men", "mi", "min", "mine", "mitt", "mot", "mykje", "mykje", "ned", "no", "noe", "noen",
    "noka", "noko", "nokon", "nokor", "nokre", "nå", "når", "og", "også", "om", "opp", "oss",
    "over", "på", "samme", "seg", "selv", "si", "sia", "sidan", "siden", "sin", "sine", "sitt",
    "sjøl", "skal", "skulle", "slik", "so", "som", "somme", "somt", "sånn", "så", "til", "um",
    "uten", "upp", "ut", "var", "vart", "varte", "ved", "vere", "verte", "vi", "vil", "ville",
    "vore", "vors", "vort", "vår", "være", "vært", "å",
];

/// English stopwords. The web is mostly English even when the question is not,
/// and a Norwegian question routinely retrieves English pages, so both sides need
/// stripping or an English page scores on `the` and `of`.
#[rustfmt::skip]
const ENGLISH_STOPWORDS: &[&str] = &[
    "a", "about", "after", "all", "also", "an", "and", "any", "are", "as", "at", "be", "been",
    "before", "being", "but", "by", "can", "could", "did", "do", "does", "for", "from", "had",
    "has", "have", "he", "her", "here", "him", "his", "how", "i", "if", "in", "into", "is", "it",
    "its", "just", "may", "me", "might", "more", "most", "must", "my", "no", "not", "now", "of",
    "on", "only", "or", "other", "our", "out", "over", "own", "same", "shall", "she", "should",
    "so", "some", "such", "than", "that", "the", "their", "them", "then", "there", "these", "they",
    "this", "those", "to", "too", "up", "us", "very", "was", "we", "were", "what", "when", "where",
    "which", "who", "whom", "why", "will", "with", "would", "you", "your",
];

/// Suffixes stripped by [`stem`], longest first.
///
/// This is a *light* stemmer, not Snowball: one suffix, minimum stem length, no
/// rewriting. It exists to make `været` match `vær` and `brukere` match `bruker`
/// — the Norwegian definite and plural inflections that would otherwise make a
/// perfectly on-topic page score zero. Anything more aggressive starts merging
/// unrelated words, and this signal is only worth 0.4 of the score when Quarry
/// has already spoken.
#[rustfmt::skip]
const STRIPPABLE_SUFFIXES: &[&str] = &[
    // 4
    "enes", "ande", "ende", // 3
    "ene", "ane", "ens", "ers", "ets", "ing", "ies", "est", "ede", // 2
    "er", "en", "et", "ar", "or", "ed", "es", "ne", "te", // 1
    "e", "s", "t", "a",
];

// ---------------------------------------------------------------------------
// Env-overridable tunables
// ---------------------------------------------------------------------------

/// Clamp an env-supplied ratio. Pure so clamping is testable without mutating
/// process env (which would make the test order-dependent).
fn clamped_f32(raw: Option<&str>, default: f32, min: f32, max: f32) -> f32 {
    raw.and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|parsed| parsed.is_finite())
        .map_or(default, |parsed| parsed.clamp(min, max))
}

/// Clamp an env-supplied count.
fn clamped_usize(raw: Option<&str>, default: usize, min: usize, max: usize) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .map_or(default, |parsed| parsed.clamp(min, max))
}

macro_rules! cached_ratio {
    ($name:ident, $env:literal, $default:expr, $min:expr, $max:expr, $doc:literal) => {
        #[doc = $doc]
        pub fn $name() -> f32 {
            static CACHED: OnceLock<f32> = OnceLock::new();
            *CACHED.get_or_init(|| {
                clamped_f32(std::env::var($env).ok().as_deref(), $default, $min, $max)
            })
        }
    };
}

cached_ratio!(
    keep_threshold,
    "VEREVON_RELEVANCE_KEEP_THRESHOLD",
    DEFAULT_KEEP_THRESHOLD,
    MIN_KEEP_THRESHOLD,
    MAX_KEEP_THRESHOLD,
    "Score at or above which a hit may be read and cited. Read once — the gate must not move mid-turn."
);
cached_ratio!(
    provider_weight,
    "VEREVON_RELEVANCE_PROVIDER_WEIGHT",
    DEFAULT_PROVIDER_WEIGHT,
    MIN_PROVIDER_WEIGHT,
    MAX_PROVIDER_WEIGHT,
    "Weight on Quarry's own reranker score when it supplied one."
);
cached_ratio!(
    soft_host_penalty,
    "VEREVON_RELEVANCE_SOFT_HOST_PENALTY",
    DEFAULT_SOFT_HOST_PENALTY,
    MIN_SOFT_HOST_PENALTY,
    MAX_SOFT_HOST_PENALTY,
    "Penalty subtracted for a soft-demoted host class. Clamped below 1.0 so the class can never become a block."
);
cached_ratio!(
    implied_domain_bonus,
    "VEREVON_RELEVANCE_IMPLIED_DOMAIN_BONUS",
    DEFAULT_IMPLIED_DOMAIN_BONUS,
    MIN_IMPLIED_DOMAIN_BONUS,
    MAX_IMPLIED_DOMAIN_BONUS,
    "Bonus added for a host the question's topic implies."
);

/// How many top-scored hits survive when the gate would drop every one.
pub fn fallback_keep() -> usize {
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        clamped_usize(
            std::env::var("VEREVON_RELEVANCE_FALLBACK_KEEP")
                .ok()
                .as_deref(),
            DEFAULT_FALLBACK_KEEP,
            MIN_FALLBACK_KEEP,
            MAX_FALLBACK_KEEP,
        )
    })
}

/// The soft-demoted host class in effect for this process.
fn soft_hosts() -> &'static [String] {
    static CACHED: OnceLock<Vec<String>> = OnceLock::new();
    CACHED.get_or_init(|| match std::env::var("VEREVON_RELEVANCE_SOFT_HOSTS") {
        // An explicitly empty override disables the class — that is a supported
        // configuration, not a parse failure to fall back from.
        Ok(raw) => raw
            .split(',')
            .map(|host| host.trim().trim_start_matches('.').to_lowercase())
            .filter(|host| !host.is_empty())
            .collect(),
        Err(_) => DEFAULT_SOFT_HOSTS
            .iter()
            .map(|host| (*host).to_owned())
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Tokenizing / stemming (pure)
// ---------------------------------------------------------------------------

/// True for a character that can be part of a word. Keeps digits (`2026`,
/// `kpi2025`) and every non-ASCII letter, so `æøå` and `ü` survive tokenizing
/// instead of splitting `været` into `v` and `ret`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// True when `word` is a stopword in either language.
fn is_stopword(word: &str) -> bool {
    NORWEGIAN_STOPWORDS.contains(&word) || ENGLISH_STOPWORDS.contains(&word)
}

/// Strip at most one inflectional suffix, refusing to go below
/// [`MIN_STEM_CHARS`].
///
/// `strip_suffix` is char-boundary safe for the ASCII suffixes in
/// [`STRIPPABLE_SUFFIXES`], so this is correct for `æøå` words.
#[must_use]
pub fn stem(word: &str) -> &str {
    if word.chars().count() <= MIN_STEM_CHARS {
        return word;
    }
    for suffix in STRIPPABLE_SUFFIXES {
        if let Some(stripped) = word.strip_suffix(suffix) {
            if stripped.chars().count() >= MIN_STEM_CHARS {
                return stripped;
            }
        }
    }
    word
}

/// Lowercase, split on non-word characters, drop stopwords, stem what remains.
fn content_stems(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    lowered
        .split(|c: char| !is_word_char(c))
        .filter(|token| !token.is_empty())
        .filter(|token| !is_stopword(token))
        .map(|token| stem(token).to_owned())
        .collect()
}

/// True when two stems name the same concept closely enough to count as a match.
///
/// Equality, or one being a prefix of the other with the shorter at least
/// [`MIN_STEM_CHARS`] long — which is what makes `vær` match `værvarsel` and
/// `vind` match `vindkraft`. It also admits some false positives (`sol` matching
/// `solidaritet`); that is a deliberate trade, because this signal is a ratio
/// over several terms and is outweighed by Quarry's score when Quarry spoke.
fn stems_match(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let (short, long) = if left.chars().count() <= right.chars().count() {
        (left, right)
    } else {
        (right, left)
    };
    short.chars().count() >= MIN_STEM_CHARS && long.starts_with(short)
}

/// Shortest topic trigger that may match as a *substring* of a term.
///
/// Norwegian writes compounds closed: the topic word `vaksine` appears inside
/// `influensavaksine`, `kraft` inside `vannkraftverk`. Prefix matching alone
/// therefore misses the compound that is the most on-topic term in the question.
/// Five characters, because at three (`lov`, `kpi`) substring matching starts
/// firing on unrelated words — those short triggers still match through
/// [`stems_match`], which is anchored at the start.
const MIN_TOPIC_SUBSTRING_CHARS: usize = 5;

/// True when `term` is a Norwegian compound built around the topic `trigger`.
///
/// Used ONLY for the implied-domain table, never for lexical overlap: a
/// substring rule is right for "is this question about energy" and much too
/// loose for "does this page mention the question's words".
fn contains_topic(term: &str, trigger: &str) -> bool {
    trigger.chars().count() >= MIN_TOPIC_SUBSTRING_CHARS && term.contains(trigger)
}

// ---------------------------------------------------------------------------
// Question
// ---------------------------------------------------------------------------

/// A question, pre-analysed once so scoring N hits does not re-tokenize it N
/// times.
#[derive(Debug, Clone)]
pub struct Question {
    /// Stemmed, deduplicated, stopword-free content terms.
    terms: Vec<String>,
    /// The lowercased raw question, kept for platform-name detection — that test
    /// must see `tiktok.com` and `tiktok` as written, not as stems.
    lowered: String,
    /// Hosts the question's topic implies, from [`IMPLIED_DOMAINS`].
    implied_hosts: BTreeSet<&'static str>,
}

impl Question {
    /// Analyse a question once.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let lowered = raw.trim().to_lowercase();
        let mut terms: Vec<String> = Vec::new();
        for stemmed in content_stems(&lowered) {
            if !terms.contains(&stemmed) {
                terms.push(stemmed);
            }
        }

        let mut implied_hosts = BTreeSet::new();
        for (trigger, hosts) in IMPLIED_DOMAINS {
            let trigger_stem = stem(trigger);
            if terms.iter().any(|term| {
                stems_match(term, trigger_stem)
                    || stems_match(term, trigger)
                    || contains_topic(term, trigger_stem)
            }) {
                implied_hosts.extend(hosts.iter().copied());
            }
        }

        Self {
            terms,
            lowered,
            implied_hosts,
        }
    }

    /// Number of content terms the gate can actually score against. Zero means
    /// the question is unjudgeable and the gate is a no-op.
    #[must_use]
    pub fn term_count(&self) -> usize {
        self.terms.len()
    }

    /// True when the question itself names this host or its platform label.
    ///
    /// This is the exemption that keeps "hvor mange brukere har `TikTok` i Norge"
    /// from having its `tiktok.com` results demoted. Both the full host and the
    /// registrable label are accepted, so "tiktok.com" and "tiktok" both count;
    /// labels shorter than [`MIN_PLATFORM_ALIAS_CHARS`] (`x.com`) are not,
    /// because a bare "x" appears in far too many questions to mean the platform.
    ///
    /// Callers must pass the **class entry** (`linkedin.com`), not the hit's full
    /// host (`no.linkedin.com`) — the label of the latter is "no", which no
    /// question would ever name.
    #[must_use]
    pub fn names_platform(&self, host: &str) -> bool {
        if self.lowered.contains(host) {
            return true;
        }
        let label = host.split('.').next().unwrap_or(host);
        if label.chars().count() < MIN_PLATFORM_ALIAS_CHARS {
            return false;
        }
        // Word-boundary match: "reddit" must not fire on "redditor-free" prose,
        // and "vk" must not fire inside "vknull".
        self.lowered
            .split(|c: char| !is_word_char(c))
            .any(|token| token == label)
    }
}

// ---------------------------------------------------------------------------
// Candidate + verdict
// ---------------------------------------------------------------------------

/// One search hit, borrowed for scoring.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub snippet: &'a str,
    /// Quarry's semantic-reranker relevance in `[0,1]`.
    ///
    /// `None` means **not reranked** — reranking is off for this deployment, or
    /// the hit fell outside the reranked head — which is a different fact from
    /// "reranked and scored zero". Conflating the two would let an unreranked
    /// response filter itself to nothing.
    pub provider_score: Option<f32>,
}

/// Why a candidate scored what it scored. Every component is reported so the
/// caller can put the reason in front of the user instead of a bare number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Verdict {
    /// Final score in `[0,1]`.
    pub score: f32,
    /// Share of the question's content terms found in title + snippet.
    pub lexical: f32,
    /// Quarry's score, when it supplied one.
    pub provider: Option<f32>,
    /// Total penalty applied for the host class (0.0 when none applied).
    pub demotion: f32,
    /// Bonus applied for an implied domain (0.0 when none applied).
    pub bonus: f32,
    /// True when the host is in the soft class AND the question did not name it.
    pub soft_host: bool,
    /// True when the question had no content terms, so relevance was not judged.
    pub unjudged: bool,
}

impl Verdict {
    /// True when this candidate may be read and cited.
    #[must_use]
    pub fn kept(&self) -> bool {
        self.score >= keep_threshold()
    }
}

/// The registrable-ish host of a URL, lowercased and `www.`-stripped. Scheme and
/// port are dropped. Empty when the URL has no host.
fn host_of(url: &str) -> String {
    let without_scheme = url
        .trim()
        .split_once("://")
        .map_or(url.trim(), |(_scheme, rest)| rest);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host)| host);
    let host = host.split(':').next().unwrap_or(host).to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_owned()
}

/// The path + query of a URL, lowercased. Used only for profile-path detection.
fn path_of(url: &str) -> String {
    let without_scheme = url
        .trim()
        .split_once("://")
        .map_or(url.trim(), |(_scheme, rest)| rest);
    without_scheme
        .find('/')
        .map_or_else(|| "/".to_owned(), |at| without_scheme[at..].to_lowercase())
}

/// True when `host` is the soft-class host `entry`, or a subdomain of it.
fn host_matches(host: &str, entry: &str) -> bool {
    host == entry || host.ends_with(&format!(".{entry}"))
}

/// Lexical overlap: the share of the question's content terms present in the
/// hit's title + snippet, with title matches worth more than snippet-only ones.
///
/// Returns `None` when the question has no content terms to score against.
fn lexical_overlap(question: &Question, title: &str, snippet: &str) -> Option<f32> {
    if question.terms.is_empty() {
        return None;
    }
    let title_stems = content_stems(title);
    let snippet_stems = content_stems(snippet);

    let mut credit = 0.0_f32;
    for term in &question.terms {
        if title_stems
            .iter()
            .any(|candidate| stems_match(term, candidate))
        {
            credit += 1.0;
        } else if snippet_stems
            .iter()
            .any(|candidate| stems_match(term, candidate))
        {
            credit += SNIPPET_ONLY_CREDIT;
        }
    }
    // reason: term counts are small; usize→f32 loses no meaningful precision
    #[allow(clippy::cast_precision_loss)]
    let denominator = question.terms.len() as f32;
    Some((credit / denominator).clamp(0.0, 1.0))
}

/// Score one candidate against one question.
///
/// The formula, in full:
///
/// ```text
/// lexical  = share of the question's content terms in title+snippet
///            (title term = 1.0, snippet-only term = SNIPPET_ONLY_CREDIT)
/// base     = provider.is_some() ? w*provider + (1-w)*lexical : lexical
/// score    = clamp(base + implied_domain_bonus - host_penalty, 0.0, 1.0)
/// ```
#[must_use]
pub fn assess(question: &Question, candidate: &Candidate<'_>) -> Verdict {
    let host = host_of(candidate.url);
    let provider = candidate
        .provider_score
        .filter(|score| score.is_finite())
        .map(|score| score.clamp(0.0, 1.0));

    let lexical = lexical_overlap(question, candidate.title, candidate.snippet);
    let unjudged = lexical.is_none();
    let lexical_value = lexical.unwrap_or(UNJUDGEABLE_SCORE);

    let base = match provider {
        Some(score) => {
            let weight = provider_weight();
            weight * score + (1.0 - weight) * lexical_value
        }
        None => lexical_value,
    };

    // Resolve which class entry matched, not merely *that* one did: the
    // exemption has to be tested against the registrable entry (`linkedin.com`),
    // never the full host. A hit on `no.linkedin.com` would otherwise be checked
    // for the label "no" and could never be exempted by a question that says
    // "linkedin".
    let matched_class = if host.is_empty() {
        None
    } else {
        soft_hosts().iter().find(|entry| host_matches(&host, entry))
    };
    // The exemption. A question that names the platform is asking about it, so
    // the class is not evidence of irrelevance for this question at all.
    let soft_host = matched_class.is_some_and(|entry| !question.names_platform(entry));
    let mut demotion = if soft_host { soft_host_penalty() } else { 0.0 };
    if soft_host {
        let path = path_of(candidate.url);
        if PROFILE_PATH_MARKERS
            .iter()
            .any(|marker| path.starts_with(marker))
        {
            demotion += PROFILE_PATH_PENALTY;
        }
    }

    let bonus = if question
        .implied_hosts
        .iter()
        .any(|implied| host_matches(&host, implied))
    {
        implied_domain_bonus()
    } else {
        0.0
    };

    Verdict {
        score: (base + bonus - demotion).clamp(0.0, 1.0),
        lexical: lexical_value,
        provider,
        demotion,
        bonus,
        soft_host,
        unjudged,
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// Which candidates survive the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepMask {
    /// One flag per input verdict, in input order.
    pub keep: Vec<bool>,
    /// True when every candidate scored below the threshold and the top-N were
    /// kept anyway. The caller MUST state this — a silently relaxed filter is
    /// the same dishonesty as a silently strict one.
    pub fallback_used: bool,
}

impl KeepMask {
    /// How many candidates survived.
    #[must_use]
    pub fn kept(&self) -> usize {
        self.keep.iter().filter(|keep| **keep).count()
    }
}

/// Apply the threshold, then the never-empty guarantee.
///
/// If nothing clears [`keep_threshold`], the highest-scoring [`fallback_keep`]
/// candidates are kept and `fallback_used` is set. Returning nothing would give
/// the user an empty Kilder tab with no explanation, which is a worse outcome
/// than a noisy one — the filter's job is to rank noise down, not to make the
/// feature disappear when it is unsure.
#[must_use]
pub fn keep_mask(verdicts: &[Verdict]) -> KeepMask {
    let keep: Vec<bool> = verdicts.iter().map(Verdict::kept).collect();
    if keep.iter().any(|k| *k) || verdicts.is_empty() {
        return KeepMask {
            keep,
            fallback_used: false,
        };
    }

    // Nothing cleared the bar. Keep the best few, in score order, ties by
    // discovery order (stable sort on a pre-ordered index list).
    let mut order: Vec<usize> = (0..verdicts.len()).collect();
    order.sort_by(|&left, &right| {
        verdicts[right]
            .score
            .partial_cmp(&verdicts[left].score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut keep = vec![false; verdicts.len()];
    for &index in order.iter().take(fallback_keep()) {
        keep[index] = true;
    }
    KeepMask {
        keep,
        fallback_used: true,
    }
}

/// One-line, user-facing explanation of why the gate dropped a candidate.
///
/// Written to be shown verbatim: it lands in the Kilder row and in the synthesis
/// prompt's uncitable list, so the user and the model both see *why* a source
/// was set aside rather than finding it silently missing.
#[must_use]
pub fn filtered_reason(verdict: &Verdict) -> String {
    let threshold = keep_threshold();
    let mut reason = format!(
        "filtered as irrelevant to the question (relevance {:.2}, below the {threshold:.2} required",
        verdict.score
    );
    if verdict.soft_host {
        reason.push_str("; social/video/aggregator host, demoted for a factual question");
    }
    if verdict.provider.is_some() {
        reason.push_str("; scored by the search reranker");
    }
    reason.push(')');
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scoring test builds candidates through this so a signature change
    /// surfaces once rather than in twenty places.
    fn candidate<'a>(url: &'a str, title: &'a str, snippet: &'a str) -> Candidate<'a> {
        Candidate {
            url,
            title,
            snippet,
            provider_score: None,
        }
    }

    fn score(question: &str, url: &str, title: &str, snippet: &str) -> f32 {
        assess(&Question::parse(question), &candidate(url, title, snippet)).score
    }

    // --- the observed live noise -----------------------------------------

    /// The exact failure this module was written for: a weather question that
    /// returned an Instagram café post, an FHI skeletal-age paper, a `TikTok`
    /// perfume video and a `LinkedIn` profile as "sources". None of them shares a
    /// single content term with the question, so all four must land under the
    /// keep threshold.
    #[test]
    fn the_observed_weather_noise_is_all_dropped() {
        let question = Question::parse("hva er været i Oslo i dag");
        let noise = [
            candidate(
                "https://www.instagram.com/p/Cx123/",
                "The best cafés in Paris",
                "Coffee, croissants and a corner table.",
            ),
            candidate(
                "https://www.fhi.no/publ/2024/skjelettalder-aldersvurdering/",
                "Skjelettalder som metode for aldersvurdering",
                "Rapport om metodens treffsikkerhet hos enslige mindreårige.",
            ),
            candidate(
                "https://www.tiktok.com/@parfyme/video/7301",
                "Min nye parfyme 🤍",
                "Denne dufter helt vilt godt.",
            ),
            candidate(
                "https://no.linkedin.com/in/ola-nordmann-1234",
                "Ola Nordmann - Senior Consultant",
                "Erfaren rådgiver innen prosjektledelse.",
            ),
        ];
        for hit in &noise {
            let verdict = assess(&question, hit);
            assert!(
                !verdict.kept(),
                "{} scored {:.2} and would have become a citation",
                hit.url,
                verdict.score
            );
        }
    }

    /// The FHI paper is the interesting one: `fhi.no` is a genuine authority, so
    /// it must be dropped on *relevance* alone with no host penalty at all —
    /// proving the gate is not just a blocklist wearing a score.
    #[test]
    fn an_authoritative_host_is_still_dropped_when_it_cannot_answer_the_question() {
        let verdict = assess(
            &Question::parse("hva er været i Oslo i dag"),
            &candidate(
                "https://www.fhi.no/publ/2024/skjelettalder/",
                "Skjelettalder som metode",
                "Rapport om aldersvurdering.",
            ),
        );
        assert_eq!(verdict.demotion, 0.0, "fhi.no is not a demoted host");
        assert!(!verdict.kept());
    }

    /// The same `fhi.no` host, asked a question it *is* the authority for, must
    /// come back — otherwise the previous test is passing for the wrong reason.
    #[test]
    fn the_same_authority_is_kept_for_the_question_it_owns() {
        let verdict = assess(
            &Question::parse("hvor mange tok influensavaksine i Norge"),
            &candidate(
                "https://www.fhi.no/sm/influensa/vaksine/vaksinasjonsdekning/",
                "Vaksinasjonsdekning for influensavaksine i Norge",
                "Andel av befolkningen som tok vaksine, per sesong.",
            ),
        );
        assert!(verdict.kept(), "scored {:.2}", verdict.score);
        assert!(verdict.bonus > 0.0, "'vaksine' implies fhi.no");
    }

    // --- demote, never block ---------------------------------------------

    /// A question that NAMES the platform legitimately wants that platform's
    /// pages. The class must not apply at all, and the hit must be kept.
    #[test]
    fn a_question_that_names_tiktok_still_gets_tiktok_results() {
        let question = Question::parse("hvor mange brukere har TikTok i Norge");
        let verdict = assess(
            &question,
            &candidate(
                "https://www.tiktok.com/business/no/insights",
                "TikTok brukertall i Norge",
                "Antall aktive brukere i Norge per 2026.",
            ),
        );
        assert!(question.names_platform("tiktok.com"));
        assert!(!verdict.soft_host, "the class must be exempt here");
        assert_eq!(verdict.demotion, 0.0);
        assert!(verdict.kept(), "scored {:.2}", verdict.score);
    }

    /// The exemption must survive a country subdomain. `no.linkedin.com` is a
    /// `LinkedIn` hit, so a question that says "linkedin" must exempt it — testing
    /// the exemption against the full host would check the label "no" and silently
    /// demote every Norwegian `LinkedIn` result on a question explicitly about
    /// `LinkedIn`.
    #[test]
    fn the_platform_exemption_survives_a_country_subdomain() {
        let verdict = assess(
            &Question::parse("hvor mange følgere har vi på LinkedIn"),
            &candidate(
                "https://no.linkedin.com/company/verevon/followers",
                "Verevon følgere på LinkedIn",
                "Antall følgere.",
            ),
        );
        assert!(!verdict.soft_host, "the class must be exempt here");
        assert_eq!(verdict.demotion, 0.0);
    }

    /// The class DEMOTES, it does not BLOCK. A social hit that genuinely answers
    /// the question survives the penalty on term coverage alone, even when the
    /// question never names the platform — so the constant list can never become
    /// a censorship list.
    #[test]
    fn a_strongly_matching_social_hit_survives_the_demotion() {
        let verdict = assess(
            &Question::parse("Rema 1000 åpningstider påsken 2026"),
            &candidate(
                "https://www.facebook.com/rema1000norge/posts/1",
                "Rema 1000 åpningstider i påsken 2026",
                "Slik er åpningstidene våre gjennom påsken 2026.",
            ),
        );
        assert!(verdict.soft_host, "the penalty was applied");
        assert!(verdict.demotion > 0.0);
        assert!(
            verdict.kept(),
            "a demoted host that answers the question must still be usable, scored {:.2}",
            verdict.score
        );
    }

    /// A `LinkedIn` *profile* takes the extra path penalty a `LinkedIn` *article*
    /// does not — a person's profile page is never the source of a fact.
    #[test]
    fn a_personal_profile_path_is_demoted_harder_than_an_article_on_the_same_host() {
        let question = Question::parse("norsk havvind kapasitet 2030 prognose");
        let profile = assess(
            &question,
            &candidate(
                "https://no.linkedin.com/in/ola-havvind",
                "Ola - havvind rådgiver",
                "Jobber med havvind kapasitet.",
            ),
        );
        let article = assess(
            &question,
            &candidate(
                "https://www.linkedin.com/pulse/havvind-kapasitet-2030",
                "Havvind kapasitet mot 2030",
                "Prognose for norsk havvind.",
            ),
        );
        assert!(profile.demotion > article.demotion);
    }

    /// The soft class is configuration, not a hardcoded judgement: an empty
    /// override must disable it entirely rather than silently falling back to the
    /// built-in list. Asserted on the pure parse so the test does not mutate
    /// process env (which a `OnceLock` would latch anyway).
    #[test]
    fn an_empty_soft_host_override_disables_the_class() {
        let parsed: Vec<String> = ""
            .split(',')
            .map(|host| host.trim().trim_start_matches('.').to_lowercase())
            .filter(|host| !host.is_empty())
            .collect();
        assert!(parsed.is_empty());
        assert!(!DEFAULT_SOFT_HOSTS.is_empty(), "the default is non-empty");
    }

    // --- Norwegian scoring ------------------------------------------------

    /// The Norwegian regression guard. `hva`, `er` and `i` are three of five
    /// words in this question; an English-only stopword list leaves them as
    /// content terms and every page on the web matches one. Only `været` and
    /// `oslo` may survive, and a met.no hit must score near the top.
    #[test]
    fn norwegian_stopwords_do_not_destroy_scoring() {
        let question = Question::parse("hva er været i Oslo");
        assert_eq!(
            question.term_count(),
            2,
            "only 'været' and 'oslo' are content terms"
        );
        let verdict = assess(
            &question,
            &candidate(
                "https://www.yr.no/nb/v%C3%A6rvarsel/daglig-tabell/1-72837/Norge/Oslo",
                "Været i Oslo - Yr",
                "Værvarsel for Oslo time for time.",
            ),
        );
        assert!(verdict.kept(), "scored {:.2}", verdict.score);
        assert!(
            verdict.lexical > 0.9,
            "both content terms are in the title, lexical was {:.2}",
            verdict.lexical
        );
    }

    /// Norwegian inflection must not cost a match. The page says `vær` and
    /// `bruker`; the question says `været` and `brukere`. Without the light
    /// stemmer both score zero and a perfectly good page is filtered.
    #[test]
    fn the_light_stemmer_matches_norwegian_definite_and_plural_forms() {
        assert_eq!(stem("været"), "vær");
        assert_eq!(stem("brukere"), "bruker");
        assert_eq!(stem("prisene"), "pris");
        // Short words are left alone rather than stemmed into nothing.
        assert_eq!(stem("vær"), "vær");
        assert_eq!(stem("oslo"), "oslo");
        // A stem is never shortened below the floor.
        assert_eq!(stem("hus"), "hus");
        assert!(stems_match("vær", "værvarsel"), "prefix match, both ways");
        assert!(stems_match("værvarsel", "vær"));
    }

    /// Norwegian and English must both work in one turn: a Norwegian question
    /// routinely retrieves English pages, and an English page whose only matches
    /// are `the` and `of` must not out-score a real one.
    #[test]
    fn english_stopwords_are_stripped_on_both_sides() {
        let question = Question::parse("what is the offshore wind capacity of Norway");
        assert!(
            question.term_count() <= 4,
            "'what/is/the/of' must be gone, got {} terms",
            question.term_count()
        );
        let real = score(
            "what is the offshore wind capacity of Norway",
            "https://www.nve.no/energi/offshore-wind/",
            "Offshore wind capacity in Norway",
            "Installed and planned offshore wind capacity.",
        );
        let filler = score(
            "what is the offshore wind capacity of Norway",
            "https://example.com/blog/the-of-and",
            "The best of the rest",
            "This is the page of a blog.",
        );
        assert!(real > filler, "real {real:.2} vs filler {filler:.2}");
    }

    // --- implied domains --------------------------------------------------

    /// A Norwegian statistics question implies `ssb.no`. At equal wording the
    /// authoritative host must rank above the generic one, which is the whole
    /// point of the bonus.
    ///
    /// The wording is deliberately partial (terms in the snippet, not the title)
    /// so neither score saturates at 1.0 — a test where both sides clamp would
    /// pass or fail for reasons unrelated to the bonus.
    #[test]
    fn an_implied_domain_hit_ranks_above_an_equally_worded_generic_one() {
        let question = "konsumprisindeksen i Norge 2026";
        let official = score(
            question,
            "https://www.ssb.no/priser-og-prisindekser/kpi",
            "Prisvekst",
            "Konsumprisindeksen for Norge steg i 2026.",
        );
        let generic = score(
            question,
            "https://someblog.example/kpi-2026",
            "Prisvekst",
            "Konsumprisindeksen for Norge steg i 2026.",
        );
        assert!(
            official > generic,
            "ssb.no {official:.2} must beat a blog {generic:.2} at equal wording"
        );
        assert!(official < 1.0, "the assertion must not be a clamp artefact");
    }

    /// Norwegian writes compounds closed, so the most on-topic term in a question
    /// is routinely a compound *containing* the topic word. Prefix matching alone
    /// misses `influensavaksine` → `vaksine` and would silently drop the
    /// implied-domain bonus on exactly the questions it is for.
    #[test]
    fn a_closed_norwegian_compound_still_triggers_its_implied_domain() {
        assert!(contains_topic("influensavaksin", "vaksin"));
        assert!(contains_topic("vannkraftverk", "vannkraft"));
        // Short triggers do NOT match as substrings — at three characters this
        // rule starts firing on unrelated words. They match via `stems_match`,
        // which is anchored at the start of the term.
        assert!(!contains_topic("lovlig", "lov"));
        assert!(!contains_topic("bekpi", "kpi"));
    }

    /// The bonus must be a preference, never an admission ticket: an SSB page
    /// with nothing to do with the question stays filtered.
    #[test]
    fn the_implied_domain_bonus_cannot_admit_an_off_topic_page() {
        let verdict = assess(
            &Question::parse("konsumprisindeksen i Norge 2026"),
            &candidate(
                "https://www.ssb.no/kultur-og-fritid/idrett-og-friluftsliv",
                "Idrett og friluftsliv",
                "Deltakelse i organisert idrett.",
            ),
        );
        assert!(verdict.bonus > 0.0, "the host is implied");
        assert!(!verdict.kept(), "but it still cannot answer the question");
    }

    // --- Quarry's own score -----------------------------------------------

    /// Quarry's reranker score is the preferred signal and must move the result.
    /// A hit the reranker liked outranks the same hit unscored.
    #[test]
    fn quarrys_reranker_score_dominates_lexical_overlap() {
        let question = Question::parse("norsk havvind utbyggingstakt");
        let page = candidate(
            "https://www.nve.no/nytt-fra-nve/rapport",
            "Rapport 2026",
            "Utbygging til havs.",
        );
        let unscored = assess(&question, &page);
        let reranked = assess(
            &question,
            &Candidate {
                provider_score: Some(0.95),
                ..page
            },
        );
        assert!(
            reranked.score > unscored.score,
            "reranked {:.2} vs unscored {:.2}",
            reranked.score,
            unscored.score
        );
        assert_eq!(reranked.provider, Some(0.95));
    }

    /// `None` must mean "not reranked", never "scored zero". If an absent score
    /// were read as 0.0 an unreranked deployment would filter itself to nothing —
    /// which is precisely how Quarry ships when `semantic_rerank` is off.
    #[test]
    fn an_absent_provider_score_is_not_a_zero_score() {
        let question = Question::parse("været i Bergen");
        let hit = candidate(
            "https://www.yr.no/nb/v%C3%A6rvarsel/Bergen",
            "Været i Bergen - Yr",
            "Værvarsel for Bergen.",
        );
        let absent = assess(&question, &hit);
        let zero = assess(
            &question,
            &Candidate {
                provider_score: Some(0.0),
                ..hit
            },
        );
        assert!(absent.kept(), "no score must not filter a good hit");
        assert!(absent.score > zero.score);
        assert_eq!(absent.provider, None);
    }

    /// A garbage provider score (NaN, out of range) must not poison the maths.
    #[test]
    fn a_malformed_provider_score_is_ignored_or_clamped() {
        let question = Question::parse("været i Oslo");
        let hit = candidate("https://www.yr.no/Oslo", "Været i Oslo", "Værvarsel.");
        let nan = assess(
            &question,
            &Candidate {
                provider_score: Some(f32::NAN),
                ..hit
            },
        );
        assert_eq!(nan.provider, None, "NaN is discarded, not propagated");
        let over = assess(
            &question,
            &Candidate {
                provider_score: Some(4.0),
                ..hit
            },
        );
        assert_eq!(over.provider, Some(1.0), "out-of-range clamps into [0,1]");
        assert!(over.score <= 1.0);
    }

    // --- unjudgeable questions -------------------------------------------

    /// With no content terms there is nothing to judge against, so the gate must
    /// become a no-op instead of guessing. A filter that cannot see must not
    /// delete.
    #[test]
    fn a_question_with_no_content_terms_filters_nothing() {
        let question = Question::parse("hva er det?");
        assert_eq!(question.term_count(), 0);
        let verdict = assess(
            &question,
            &candidate("https://example.com/x", "Noe", "Noe tekst."),
        );
        assert!(verdict.unjudged);
        assert!(verdict.kept());
    }

    // --- the gate ---------------------------------------------------------

    /// The never-empty guarantee. If every candidate is below the bar, the top
    /// few survive and the caller is told the fallback fired — an empty Kilder
    /// tab with no explanation is a worse outcome than a noisy one.
    #[test]
    fn the_gate_never_filters_everything_away() {
        let low = |score: f32| Verdict {
            score,
            lexical: score,
            provider: None,
            demotion: 0.0,
            bonus: 0.0,
            soft_host: false,
            unjudged: false,
        };
        let verdicts = vec![low(0.02), low(0.20), low(0.11), low(0.05), low(0.19)];
        let mask = keep_mask(&verdicts);
        assert!(mask.fallback_used);
        assert_eq!(mask.kept(), fallback_keep());
        // The survivors are the best-scoring ones, not the first-discovered.
        assert!(mask.keep[1], "0.20 is the top score");
        assert!(mask.keep[4], "0.19 is second");
        assert!(!mask.keep[0], "0.02 is not");
    }

    /// The fallback must NOT fire when anything cleared the bar — otherwise it
    /// would quietly widen the gate on every healthy turn.
    #[test]
    fn the_fallback_stays_off_when_something_clears_the_threshold() {
        let verdict = |score: f32| Verdict {
            score,
            lexical: score,
            provider: None,
            demotion: 0.0,
            bonus: 0.0,
            soft_host: false,
            unjudged: false,
        };
        let mask = keep_mask(&[verdict(0.9), verdict(0.01)]);
        assert!(!mask.fallback_used);
        assert_eq!(mask.keep, vec![true, false]);
    }

    /// An empty candidate list is an empty mask, not a fallback: there is nothing
    /// to relax the gate for, and claiming the fallback fired would be a false
    /// statement in the coverage line.
    #[test]
    fn an_empty_candidate_list_does_not_trip_the_fallback() {
        let mask = keep_mask(&[]);
        assert!(!mask.fallback_used);
        assert_eq!(mask.kept(), 0);
    }

    // --- thresholds -------------------------------------------------------

    /// Env overrides are clamped, so a fat-fingered `2.0` cannot delete every
    /// source and a negative value cannot disable the gate by underflow.
    #[test]
    fn env_thresholds_are_clamped_to_their_documented_range() {
        assert_eq!(
            clamped_f32(Some("2.0"), 0.3, MIN_KEEP_THRESHOLD, MAX_KEEP_THRESHOLD),
            MAX_KEEP_THRESHOLD
        );
        assert_eq!(
            clamped_f32(Some("-1"), 0.3, MIN_KEEP_THRESHOLD, MAX_KEEP_THRESHOLD),
            MIN_KEEP_THRESHOLD
        );
        assert_eq!(
            clamped_f32(Some("NaN"), 0.3, MIN_KEEP_THRESHOLD, MAX_KEEP_THRESHOLD),
            0.3,
            "a non-finite override falls back to the default"
        );
        assert_eq!(clamped_f32(Some("nonsense"), 0.3, 0.0, 0.8), 0.3);
        assert_eq!(clamped_f32(None, 0.3, 0.0, 0.8), 0.3);
        // The host penalty can never reach 1.0, which is what keeps the soft
        // class a demotion rather than a block.
        assert!(MAX_SOFT_HOST_PENALTY < 1.0);
        assert_eq!(
            clamped_f32(
                Some("1.0"),
                0.35,
                MIN_SOFT_HOST_PENALTY,
                MAX_SOFT_HOST_PENALTY
            ),
            MAX_SOFT_HOST_PENALTY
        );
        assert_eq!(
            clamped_usize(Some("999"), 3, MIN_FALLBACK_KEEP, MAX_FALLBACK_KEEP),
            MAX_FALLBACK_KEEP
        );
        assert_eq!(
            clamped_usize(Some("0"), 3, MIN_FALLBACK_KEEP, MAX_FALLBACK_KEEP),
            MIN_FALLBACK_KEEP
        );
    }

    // --- host parsing -----------------------------------------------------

    /// Host extraction has to survive real URLs, because a mis-parsed host means
    /// a demotion applied to the wrong page.
    #[test]
    fn host_parsing_handles_subdomains_ports_and_bare_urls() {
        assert_eq!(host_of("https://www.instagram.com/p/x/"), "instagram.com");
        assert_eq!(
            host_of("http://NO.LinkedIn.com:8080/in/a"),
            "no.linkedin.com"
        );
        assert_eq!(host_of("ssb.no/kpi"), "ssb.no");
        assert_eq!(host_of(""), "");
        assert!(host_matches("no.linkedin.com", "linkedin.com"), "subdomain");
        assert!(host_matches("linkedin.com", "linkedin.com"));
        assert!(
            !host_matches("notlinkedin.com", "linkedin.com"),
            "suffix match must respect the dot boundary"
        );
        assert_eq!(path_of("https://x.com/in/a?b=c"), "/in/a?b=c");
        assert_eq!(path_of("https://x.com"), "/");
    }

    /// The platform exemption must be a word match, not a substring match, or
    /// unrelated prose would disable the class at random.
    #[test]
    fn the_platform_exemption_requires_a_real_word_match() {
        assert!(Question::parse("tiktok annonsering").names_platform("tiktok.com"));
        assert!(Question::parse("hva skjer på tiktok.com").names_platform("tiktok.com"));
        assert!(!Question::parse("hva er været i Oslo").names_platform("tiktok.com"));
        // `x.com`'s label is one character; a bare "x" must never exempt it.
        assert!(!Question::parse("løs for x i likningen").names_platform("x.com"));
        assert!(
            Question::parse("hvor mange følgere har vi på twitter").names_platform("twitter.com"),
            "the readable alias still works"
        );
    }

    // --- reason text ------------------------------------------------------

    /// The reason is shown verbatim to the user and to the model, so it must name
    /// the score, the bar, and the host class when that is why.
    #[test]
    fn the_filtered_reason_states_the_score_the_bar_and_the_host_class() {
        let verdict = assess(
            &Question::parse("hva er været i Oslo"),
            &candidate("https://www.instagram.com/p/Cx/", "Paris cafés", "Coffee."),
        );
        let reason = filtered_reason(&verdict);
        assert!(reason.contains("filtered as irrelevant"), "{reason}");
        assert!(reason.contains("0.00"), "{reason}");
        assert!(reason.contains("social/video/aggregator host"), "{reason}");
    }
}
