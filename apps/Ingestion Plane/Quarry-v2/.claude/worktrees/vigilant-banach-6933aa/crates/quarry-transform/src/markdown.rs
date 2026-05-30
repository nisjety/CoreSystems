//! HTML → markdown via html2md (Phase-1 implementation; readability-first pipeline tracked in roadmap).

pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}
