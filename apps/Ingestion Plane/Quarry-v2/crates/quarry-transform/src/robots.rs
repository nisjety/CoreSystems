//! robots.txt parser.
//!
//! Minimal RFC 9309 parse: `User-agent`, `Allow`, `Disallow`, `Sitemap`,
//! `Crawl-delay`. Longest-matching-prefix wins; `Allow` beats `Disallow` at
//! equal length (RFC convention).
//!
//! Donor: V1's robots check was URL-string contains; this is a real parser.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RobotsTxt {
    pub groups: HashMap<String, GroupRules>,
    pub sitemaps: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupRules {
    pub allow: Vec<String>,
    pub disallow: Vec<String>,
    pub crawl_delay_s: Option<f64>,
}

impl RobotsTxt {
    pub fn parse(input: &str) -> Self {
        let mut out = RobotsTxt::default();
        let mut current_agents: Vec<String> = Vec::new();
        let mut after_agents = false;

        for raw in input.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();

            match key.as_str() {
                "user-agent" => {
                    if after_agents {
                        // New group block — reset
                        current_agents.clear();
                        after_agents = false;
                    }
                    current_agents.push(value.to_ascii_lowercase());
                    // A named group exists even with no rule under it.
                    out.groups
                        .entry(value.to_ascii_lowercase())
                        .or_default();
                }
                "allow" => {
                    if !value.is_empty() {
                        for ua in &current_agents {
                            out.groups
                                .entry(ua.clone())
                                .or_default()
                                .allow
                                .push(value.clone());
                        }
                        after_agents = true;
                    }
                }
                "disallow" => {
                    // Empty Disallow means allow-all; record the group anyway
                    for ua in &current_agents {
                        let group = out.groups.entry(ua.clone()).or_default();
                        if !value.is_empty() {
                            group.disallow.push(value.clone());
                        }
                    }
                    after_agents = true;
                }
                "crawl-delay" => {
                    if let Ok(n) = value.parse::<f64>() {
                        for ua in &current_agents {
                            out.groups.entry(ua.clone()).or_default().crawl_delay_s = Some(n);
                        }
                    }
                    after_agents = true;
                }
                "sitemap" => {
                    out.sitemaps.push(value);
                }
                _ => {}
            }
        }
        out
    }

    /// Resolve the rules for a given user-agent token (case-insensitive).
    /// Falls back to `*` when no specific rules are found.
    pub fn rules_for<'a>(&'a self, user_agent: &str) -> Option<&'a GroupRules> {
        let lc = user_agent.to_ascii_lowercase();
        self.groups.get(&lc).or_else(|| self.groups.get("*"))
    }

    /// Check whether `path` is allowed for `user_agent`.
    /// Implements the longest-match-wins rule: longer Allow > longer Disallow.
    pub fn is_allowed(&self, user_agent: &str, path: &str) -> bool {
        let Some(rules) = self.rules_for(user_agent) else {
            return true;
        };
        let allow_match = longest_prefix_match(&rules.allow, path);
        let disallow_match = longest_prefix_match(&rules.disallow, path);

        match (allow_match, disallow_match) {
            (None, None) => true,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(a), Some(d)) => a >= d, // Allow wins on tie or when longer
        }
    }
}

impl RobotsTxt {
    /// Check `path_and_query` against every product token and allow it only when
    /// every token allows it: the most restrictive answer wins. `path_and_query`
    /// is the URL's path followed by `?query` when there is one (RFC 9309 2.2.2
    /// matches the rule against both). A token with no group of its own falls
    /// back to `*`, so a file that names only `*` is read once per token and
    /// gives the same answer.
    pub fn is_allowed_any(&self, tokens: &[&str], path_and_query: &str) -> bool {
        self.denying_token(tokens, path_and_query).is_none()
    }

    /// The first token (in the order given) whose rules deny `path_and_query`,
    /// or `None` when every token allows it.
    pub fn denying_token<'t>(&self, tokens: &[&'t str], path_and_query: &str) -> Option<&'t str> {
        tokens
            .iter()
            .copied()
            .find(|token| !self.is_allowed(token, path_and_query))
    }

    /// The longest `Crawl-delay` any of the tokens' groups asks for, in seconds.
    /// Negative, NaN and infinite values read as no delay.
    pub fn crawl_delay_any(&self, tokens: &[&str]) -> Option<f64> {
        tokens
            .iter()
            .filter_map(|token| self.rules_for(token).and_then(|r| r.crawl_delay_s))
            .filter(|d| d.is_finite() && *d > 0.0)
            .fold(None, |best: Option<f64>, d| Some(best.map_or(d, |b| b.max(d))))
    }
}

/// The path of `url` followed by `?query` when it has one: the string a robots
/// rule is matched against.
pub fn path_and_query(url: &url::Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    }
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn longest_prefix_match(rules: &[String], path: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    for r in rules {
        if path_matches(r, path) {
            let len = r.len();
            best = Some(best.map(|b| b.max(len)).unwrap_or(len));
        }
    }
    best
}

fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern == "/" {
        return true;
    }
    let (core, anchored) = match pattern.strip_suffix('$') {
        Some(stripped) => (stripped, true),
        None => (pattern, false),
    };
    if core.contains('*') {
        // wildcard_match is an anchored full-string match; behaves correctly
        // both with and without `$`.
        return wildcard_match(core, path);
    }
    if anchored {
        path == core
    } else {
        path.starts_with(core)
    }
}

fn wildcard_match(pattern: &str, path: &str) -> bool {
    let mut p_idx = 0usize;
    let mut path_idx = 0usize;
    let mut star_p: Option<usize> = None;
    let mut star_path: usize = 0;
    let p = pattern.as_bytes();
    let s = path.as_bytes();

    while path_idx < s.len() {
        if p_idx < p.len() && p[p_idx] == s[path_idx] {
            p_idx += 1;
            path_idx += 1;
        } else if p_idx < p.len() && p[p_idx] == b'*' {
            star_p = Some(p_idx);
            star_path = path_idx;
            p_idx += 1;
        } else if let Some(sp) = star_p {
            p_idx = sp + 1;
            star_path += 1;
            path_idx = star_path;
        } else {
            return false;
        }
    }
    while p_idx < p.len() && p[p_idx] == b'*' {
        p_idx += 1;
    }
    p_idx == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_rules() {
        let txt = "User-agent: *\nDisallow: /admin\nAllow: /admin/public\nCrawl-delay: 5\n";
        let r = RobotsTxt::parse(txt);
        let rules = r.rules_for("Quarry").unwrap();
        assert_eq!(rules.disallow, vec!["/admin"]);
        assert_eq!(rules.allow, vec!["/admin/public"]);
        assert_eq!(rules.crawl_delay_s, Some(5.0));
    }

    #[test]
    fn allow_beats_disallow_when_longer() {
        let txt = "User-agent: *\nDisallow: /admin\nAllow: /admin/public\n";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("Quarry", "/admin"));
        assert!(r.is_allowed("Quarry", "/admin/public"));
        assert!(r.is_allowed("Quarry", "/admin/public/page"));
        assert!(!r.is_allowed("Quarry", "/admin/private"));
    }

    #[test]
    fn unknown_user_agent_falls_back_to_star() {
        let txt = "User-agent: *\nDisallow: /private\n";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("RandomBot", "/private/data"));
        assert!(r.is_allowed("RandomBot", "/public"));
    }

    #[test]
    fn specific_user_agent_overrides_star() {
        let txt = "User-agent: Quarry\nDisallow: /q-only\n\nUser-agent: *\nDisallow: /private\n";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("Quarry", "/q-only"));
        // Quarry has its own group, doesn't inherit /private
        assert!(r.is_allowed("Quarry", "/private"));
    }

    #[test]
    fn empty_disallow_means_allow_all() {
        let txt = "User-agent: *\nDisallow:\n";
        let r = RobotsTxt::parse(txt);
        assert!(r.is_allowed("Quarry", "/anything"));
    }

    #[test]
    fn sitemaps_collected() {
        let txt =
            "Sitemap: https://example.com/sitemap.xml\nSitemap: https://example.com/sitemap2.xml\n";
        let r = RobotsTxt::parse(txt);
        assert_eq!(r.sitemaps.len(), 2);
        assert!(r.sitemaps[0].ends_with("sitemap.xml"));
    }

    #[test]
    fn comments_are_stripped() {
        let txt = "# header\nUser-agent: * # all bots\nDisallow: /a # secret\n";
        let r = RobotsTxt::parse(txt);
        assert_eq!(r.rules_for("Quarry").unwrap().disallow, vec!["/a"]);
    }

    #[test]
    fn wildcard_match_works() {
        assert!(wildcard_match("/admin/*", "/admin/users"));
        assert!(!wildcard_match("/admin/*", "/public"));
        // path_matches handles `$` anchoring; wildcard_match itself does not.
        assert!(path_matches("/*.pdf$", "/foo.pdf"));
        assert!(!path_matches("/*.pdf$", "/foo.pdf.html"));
    }

    #[test]
    fn question_mark_is_a_literal_not_a_wildcard() {
        assert!(!wildcard_match("/a?c*", "/abc"));
        assert!(wildcard_match("/a?c*", "/a?cd"));
    }

    #[test]
    fn query_aware_rules_match_path_and_query() {
        let r = RobotsTxt::parse("User-agent: *\nDisallow: /*?session=\nDisallow: /search$\n");
        assert!(!r.is_allowed("quarry", "/page?session=1"));
        assert!(!r.is_allowed("quarry", "/a/b?x=1&session=2"));
        assert!(r.is_allowed("quarry", "/page?id=1"));
        // `$` anchors the end of path and query together.
        assert!(!r.is_allowed("quarry", "/search"));
        assert!(r.is_allowed("quarry", "/search?q=x"));
    }

    #[test]
    fn is_allowed_any_takes_the_most_restrictive_token() {
        let r = RobotsTxt::parse(
            "User-agent: quarrybot\nDisallow: /a\n\nUser-agent: quarry\nDisallow: /b\n\nUser-agent: *\nDisallow: /c\n",
        );
        let tokens = ["quarrybot", "quarry"];
        assert!(!r.is_allowed_any(&tokens, "/a"));
        assert!(!r.is_allowed_any(&tokens, "/b"));
        // Each token has its own group, so `*` does not apply to either.
        assert!(r.is_allowed_any(&tokens, "/c"));
        assert_eq!(r.denying_token(&tokens, "/b"), Some("quarry"));
        assert_eq!(r.denying_token(&tokens, "/a"), Some("quarrybot"));
        assert_eq!(r.denying_token(&tokens, "/z"), None);
    }

    #[test]
    fn star_group_applies_to_every_token_without_its_own_group() {
        let r = RobotsTxt::parse("User-agent: *\nDisallow: /private\n");
        assert!(!r.is_allowed_any(&["quarrybot", "quarry"], "/private/x"));
        assert!(r.is_allowed_any(&["quarrybot", "quarry"], "/public"));
    }

    #[test]
    fn a_named_group_with_no_rules_allows_everything() {
        let r = RobotsTxt::parse("User-agent: quarrybot\n\nUser-agent: *\nDisallow: /\n");
        assert!(r.is_allowed("quarrybot", "/anything"));
        assert!(!r.is_allowed("other", "/anything"));
    }

    #[test]
    fn crawl_delay_any_takes_the_longest_and_ignores_garbage() {
        let r = RobotsTxt::parse(
            "User-agent: quarrybot\nCrawl-delay: 2\n\nUser-agent: quarry\nCrawl-delay: 5\n",
        );
        assert_eq!(r.crawl_delay_any(&["quarrybot", "quarry"]), Some(5.0));
        let neg = RobotsTxt::parse("User-agent: *\nCrawl-delay: -3\n");
        assert_eq!(neg.crawl_delay_any(&["quarry"]), None);
        let none = RobotsTxt::parse("User-agent: *\nDisallow: /x\n");
        assert_eq!(none.crawl_delay_any(&["quarry"]), None);
    }

    #[test]
    fn longest_match_and_allow_wins_a_tie() {
        let r = RobotsTxt::parse("User-agent: *\nDisallow: /p\nAllow: /p\n");
        assert!(r.is_allowed("quarry", "/p/x"));
        let r = RobotsTxt::parse("User-agent: *\nDisallow: /p/secret\nAllow: /p\n");
        assert!(!r.is_allowed("quarry", "/p/secret/a"));
        assert!(r.is_allowed("quarry", "/p/open"));
    }

    #[test]
    fn rules_before_any_user_agent_line_are_ignored() {
        let r = RobotsTxt::parse("Disallow: /x\nUser-agent: *\nDisallow: /y\n");
        assert!(r.is_allowed("quarry", "/x"));
        assert!(!r.is_allowed("quarry", "/y"));
    }

    #[test]
    fn groups_for_the_same_agent_merge() {
        let r = RobotsTxt::parse(
            "User-agent: quarry\nDisallow: /a\n\nUser-agent: other\nDisallow: /o\n\nUser-agent: quarry\nDisallow: /b\n",
        );
        assert!(!r.is_allowed("quarry", "/a"));
        assert!(!r.is_allowed("quarry", "/b"));
    }

    #[test]
    fn path_and_query_joins_query_when_present() {
        let u = url::Url::parse("https://example.test/a/b?x=1&y=2#frag").unwrap();
        assert_eq!(path_and_query(&u), "/a/b?x=1&y=2");
        let u = url::Url::parse("https://example.test/a/b").unwrap();
        assert_eq!(path_and_query(&u), "/a/b");
    }

    #[test]
    fn dollar_anchors_path_end() {
        let txt = "User-agent: *\nDisallow: /file.pdf$\n";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("Quarry", "/file.pdf"));
        assert!(r.is_allowed("Quarry", "/file.pdf.html"));
    }
}
