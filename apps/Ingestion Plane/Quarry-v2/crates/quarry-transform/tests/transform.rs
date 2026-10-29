//! Transform pipeline tests: fingerprint, diff, links, markdown, metadata, chunk.

use quarry_core::output::ChangeStatus;
use quarry_transform::{chunk, diff, fingerprint, links, markdown, metadata};
use url::Url;

#[test]
fn fingerprint_stable_and_prefixed() {
    let a = fingerprint::content_fingerprint(b"hello world");
    let b = fingerprint::content_fingerprint(b"hello world");
    assert_eq!(a, b);
    assert!(a.as_str().starts_with("blake3:"));
}

#[test]
fn fingerprint_sensitive_to_bytes() {
    let a = fingerprint::content_fingerprint(b"hello world");
    let b = fingerprint::content_fingerprint(b"hello worlx");
    assert_ne!(a, b);
}

#[test]
fn text_fingerprint_ignores_whitespace_and_case() {
    let a = fingerprint::text_fingerprint("Hello   World");
    let b = fingerprint::text_fingerprint("hello world");
    assert_eq!(a, b, "normalized text should collapse ws + case");
}

#[test]
fn diff_new_changed_unchanged() {
    let fp1 = fingerprint::content_fingerprint(b"one");
    let fp2 = fingerprint::content_fingerprint(b"two");
    assert!(matches!(diff::compare(None, &fp1), ChangeStatus::New));
    assert!(matches!(
        diff::compare(Some(&fp1), &fp1),
        ChangeStatus::Unchanged
    ));
    assert!(matches!(
        diff::compare(Some(&fp1), &fp2),
        ChangeStatus::Changed
    ));
}

#[test]
fn links_resolve_relative_against_base() {
    let base = Url::parse("https://example.com/page/").unwrap();
    let html = r#"
        <a href="/abs">abs</a>
        <a href="rel">rel</a>
        <a href="https://other.com/x" rel="noopener">other</a>
    "#;
    let out = links::extract(html, &base);
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].href, "https://example.com/abs");
    assert_eq!(out[1].href, "https://example.com/page/rel");
    assert_eq!(out[2].href, "https://other.com/x");
    assert_eq!(out[2].rel.as_deref(), Some("noopener"));
}

#[test]
fn links_skip_missing_href() {
    let base = Url::parse("https://example.com/").unwrap();
    let html = r#"<a>no href</a><a href="/x">ok</a>"#;
    let out = links::extract(html, &base);
    assert_eq!(out.len(), 1);
}

#[test]
fn markdown_converts_basic_html() {
    let md = markdown::html_to_markdown("<h1>Title</h1><p>Body</p>");
    assert!(md.contains("Title"));
    assert!(md.contains("Body"));
}

#[test]
fn metadata_extracts_title_and_lang() {
    let meta = metadata::extract(
        r#"<!doctype html><html lang="no"><head><title>  Hi  </title></head><body/></html>"#,
        Some("text/html".into()),
    );
    assert_eq!(meta.title.as_deref(), Some("Hi"));
    assert_eq!(meta.lang.as_deref(), Some("no"));
    assert_eq!(meta.content_type.as_deref(), Some("text/html"));
}

#[test]
fn chunk_paragraph_splits_and_respects_max() {
    let text = "alpha\n\nbeta beta\n\ngamma gamma gamma";
    let chunks = chunk::paragraph_chunks(text, 100);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].text, "alpha");
    assert_eq!(chunks[2].text, "gamma gamma gamma");
}

#[test]
fn chunk_long_paragraph_slices_hard() {
    let big = "x".repeat(250);
    let chunks = chunk::paragraph_chunks(&big, 100);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].text.len(), 100);
    assert_eq!(chunks[2].text.len(), 50);
}

/// Strict-mode determinism roundtrip: the entire transform pipeline (HTML →
/// readable extract → markdown → links → metadata → fingerprint) must be
/// byte-identical across two consecutive runs over identical input. This
/// catches sneaky non-determinism (HashMap iteration order, time-based fields,
/// RNG seeded from entropy) that the per-stage tests can miss.
#[test]
fn strict_determinism_roundtrip() {
    use quarry_transform::{determinism, html_to_readable_markdown};

    const HTML: &str = r#"
<!doctype html><html lang="en"><head>
  <title>Strict Mode</title>
  <meta name="author" content="Quarry">
</head><body>
  <nav><a href="/a">A</a><a href="/b">B</a></nav>
  <main>
    <article>
      <h1>Strict Mode</h1>
      <p>Quarry V2 must produce byte-identical artifacts across reruns of the
         same input under the strict determinism contract. This paragraph is
         long enough to exceed the readability scoring threshold so the article
         body is the chosen subtree, not the surrounding navigation chrome.</p>
      <p>Two paragraphs is plenty for the scorer; the rest of the page is
         intentionally noisy so we exercise the noise-stripping code path.</p>
    </article>
  </main>
  <footer>copyright</footer>
</body></html>"#;

    // Pipeline: extract → markdown → links → metadata → fingerprint.
    let pipeline = || -> Vec<u8> {
        let md = html_to_readable_markdown(HTML);
        let base = Url::parse("https://example.test/page").unwrap();
        let link_list = links::extract(HTML, &base);
        let meta = metadata::extract(HTML, Some("text/html".into()));
        let mut buf = String::new();
        buf.push_str(&md);
        buf.push('\n');
        buf.push_str(&format!("links={}\n", link_list.len()));
        if let Some(t) = meta.title.as_deref() {
            buf.push_str(&format!("title={t}\n"));
        }
        if let Some(l) = meta.lang.as_deref() {
            buf.push_str(&format!("lang={l}\n"));
        }
        buf.into_bytes()
    };

    let fp = determinism::verify_deterministic(pipeline)
        .expect("transform pipeline must be deterministic across reruns");
    assert!(fp.as_str().starts_with("blake3:"));

    // Sanity: a third invocation must match too. (Catches state baked across
    // calls, e.g. lazy-init that randomizes on second run.)
    let third = fingerprint::content_fingerprint(&pipeline());
    assert_eq!(fp, third, "third pipeline invocation drifted");
}
