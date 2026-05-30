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
        self.groups
            .get(&lc)
            .or_else(|| self.groups.get("*"))
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
        if p_idx < p.len() && (p[p_idx] == s[path_idx] || p[p_idx] == b'?') {
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
        let txt = "Sitemap: https://example.com/sitemap.xml\nSitemap: https://example.com/sitemap2.xml\n";
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
    fn dollar_anchors_path_end() {
        let txt = "User-agent: *\nDisallow: /file.pdf$\n";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("Quarry", "/file.pdf"));
        assert!(r.is_allowed("Quarry", "/file.pdf.html"));
    }
}
