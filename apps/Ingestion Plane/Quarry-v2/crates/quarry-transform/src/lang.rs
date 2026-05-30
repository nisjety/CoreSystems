//! Body-content language detection.
//!
//! `html[lang]` is the polite first source — when the publisher tags
//! their HTML correctly, we use it. But a lot of multilingual sites
//! ship a single `lang="en"` on the root regardless of the page's
//! actual content (because the CMS templating engine doesn't know
//! per-page), and geographic-CDN routing can serve a Norwegian page
//! with an `en-US` lang tag on accident. Downstream LLM extraction
//! gets confused — it prompts the model in the wrong language.
//!
//! `whatlang` is a statistical detector that maps Unicode codepoint
//! frequencies to a language code (ISO 639-3). It's fast (~20µs for
//! 4 KB of text), zero-allocation in the hot path, and accurate above
//! ~50 chars of content. Below that we don't try — sub-50-char
//! snippets are usually navigation chrome and we'd be guessing.

use whatlang::Detector;

/// Minimum visible-text length before we attempt detection. Short
/// strings get false positives — "Hello world" can match dozens of
/// Romance languages. Conservative on purpose.
const MIN_TEXT_BYTES: usize = 50;

/// Detection result. `code` is ISO 639-3 (e.g. `nob` for Norwegian
/// Bokmål, `eng` for English, `deu` for German). `confidence` is
/// roughly 0.0 → 1.0; we only return Some when the detector is at
/// least somewhat confident.
#[derive(Debug, Clone, PartialEq)]
pub struct LangDetect {
    pub code: &'static str,
    pub confidence: f64,
}

/// Detect the dominant natural-language of `text`. Returns `None`
/// when the input is too short OR confidence is below a useful
/// threshold.
///
/// Caller should pass cleaned visible text — i.e. the readability-
/// extracted body, not raw HTML with tags and script content. Tag
/// soup confuses the detector.
pub fn detect(text: &str) -> Option<LangDetect> {
    if text.len() < MIN_TEXT_BYTES {
        return None;
    }
    let info = Detector::new().detect(text)?;
    if !info.is_reliable() {
        return None;
    }
    let code = info.lang().code();
    Some(LangDetect {
        code,
        confidence: info.confidence(),
    })
}

/// Map the detector's ISO 639-3 code to the more common ISO 639-1
/// two-letter code when one exists (e.g. `nob` → `nb`, `eng` → `en`).
/// Returns the original 3-letter code when no mapping is defined.
///
/// Useful when callers want to emit `html[lang]`-style values or
/// match against the user's UI language preference (which is usually
/// 639-1 in browsers).
pub fn iso_639_1(code_3: &str) -> &str {
    match code_3 {
        "nob" => "nb",
        "nno" => "nn",
        "swe" => "sv",
        "dan" => "da",
        "eng" => "en",
        "deu" => "de",
        "fra" => "fr",
        "spa" => "es",
        "ita" => "it",
        "nld" => "nl",
        "por" => "pt",
        "pol" => "pl",
        "rus" => "ru",
        "ukr" => "uk",
        "ces" => "cs",
        "fin" => "fi",
        "tur" => "tr",
        "ara" => "ar",
        "heb" => "he",
        "hin" => "hi",
        "jpn" => "ja",
        "kor" => "ko",
        "zho" => "zh",
        // schema.org-style: pass through unknown codes as-is so
        // consumers can still log/store them.
        other => other,
    }
}

/// Whether a 639-3 code is one of the Nordic languages we explicitly
/// care about. Useful as a quick gate for downstream routing.
pub fn is_nordic(code_3: &str) -> bool {
    matches!(code_3, "nob" | "nno" | "swe" | "dan" | "isl" | "fao")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_norwegian() {
        let text = "Velkommen til vår nettbutikk. Vi tilbyr et bredt utvalg av koreanske hudpleieprodukter til konkurransedyktige priser.";
        let d = detect(text).expect("should detect");
        assert!(matches!(d.code, "nob" | "nno"));
        assert_eq!(iso_639_1(d.code), if d.code == "nob" { "nb" } else { "nn" });
    }

    #[test]
    fn detects_english() {
        let text = "Welcome to our shop. We offer a wide selection of premium products at competitive prices.";
        let d = detect(text).expect("should detect");
        assert_eq!(d.code, "eng");
        assert_eq!(iso_639_1(d.code), "en");
    }

    #[test]
    fn detects_korean() {
        let text = "안녕하세요. 저희 매장에 오신 것을 환영합니다. 다양한 한국 화장품을 합리적인 가격에 제공합니다. 더 많은 제품을 둘러보세요.";
        let d = detect(text).expect("should detect");
        assert_eq!(d.code, "kor");
        assert_eq!(iso_639_1(d.code), "ko");
    }

    #[test]
    fn short_input_returns_none() {
        assert!(detect("hi").is_none());
        assert!(detect("").is_none());
    }

    #[test]
    fn iso_mapping_falls_through_for_unknown() {
        assert_eq!(iso_639_1("xxx"), "xxx");
    }

    #[test]
    fn nordic_helper() {
        assert!(is_nordic("nob"));
        assert!(is_nordic("swe"));
        assert!(!is_nordic("eng"));
    }
}
