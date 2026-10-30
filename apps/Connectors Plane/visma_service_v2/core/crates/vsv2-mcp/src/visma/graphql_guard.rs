//! Keeps DataMart queries read-only. DataMart itself is read-only (decision: writes never route
//! through it), so this is defense in depth against a future mutation-capable endpoint.

use thiserror::Error;

pub const MAX_QUERY_CHARS: usize = 20_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GqlGuardError {
    #[error("query is empty")]
    Empty,
    #[error("query is longer than {MAX_QUERY_CHARS} characters")]
    TooLong,
    #[error("`{0}` operations are not allowed; only queries")]
    NotReadOnly(String),
    #[error("unterminated string")]
    Unterminated,
}

/// Blank out strings (`"..."`, `"""..."""`) and `#` comments, then reject any `mutation` /
/// `subscription` word.
pub fn validate_query(q: &str) -> Result<(), GqlGuardError> {
    let q = q.trim();
    if q.is_empty() {
        return Err(GqlGuardError::Empty);
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(GqlGuardError::TooLong);
    }
    let c: Vec<char> = q.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '#' => {
                while i < c.len() && c[i] != '\n' && c[i] != '\r' {
                    i += 1;
                }
                out.push(' ');
            }
            '"' => {
                let block = c.get(i + 1) == Some(&'"') && c.get(i + 2) == Some(&'"');
                i += if block { 3 } else { 1 };
                loop {
                    if i >= c.len() {
                        return Err(GqlGuardError::Unterminated);
                    }
                    if c[i] == '\\' && !block {
                        i += 2;
                        continue;
                    }
                    if block && c[i] == '\\' && c.get(i + 1) == Some(&'"') {
                        i += 2;
                        continue;
                    }
                    let closes = c[i] == '"'
                        && (!block || (c.get(i + 1) == Some(&'"') && c.get(i + 2) == Some(&'"')));
                    if closes {
                        i += if block { 3 } else { 1 };
                        break;
                    }
                    i += 1;
                }
                out.push(' ');
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    for w in out.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')) {
        let lw = w.to_ascii_lowercase();
        if lw == "mutation" || lw == "subscription" {
            return Err(GqlGuardError::NotReadOnly(lw));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_queries_including_words_in_strings_and_comments() {
        for q in [
            "{ customers(first: 5) { nodes { id } } }",
            "query Q($n: Int) { customers(first: $n) { id } }",
            r#"{ a(filter: "mutation") { id } }"#,
            "# mutation in a comment\n{ a { id } }",
            "{ a(d: \"\"\"subscription\nmutation\"\"\") { id } }",
            "{ __schema { queryType { name } } }",
        ] {
            assert_eq!(validate_query(q), Ok(()), "{q}");
        }
    }

    #[test]
    fn rejects_mutations_and_subscriptions_in_any_position() {
        for q in [
            "mutation { deleteAll }",
            "  MUTATION M { x }",
            "query A { a { id } }\nmutation B { b }",
            "subscription { onChange { id } }",
        ] {
            assert!(
                matches!(validate_query(q), Err(GqlGuardError::NotReadOnly(_))),
                "{q}"
            );
        }
    }

    #[test]
    fn carriage_return_ends_a_comment_like_graphql_servers_do() {
        let q = "{ a { id } } # harmless\rmutation { x }";
        assert!(matches!(
            validate_query(q),
            Err(GqlGuardError::NotReadOnly(_))
        ));
    }

    #[test]
    fn rejects_empty_long_and_unterminated() {
        assert_eq!(validate_query("  "), Err(GqlGuardError::Empty));
        assert_eq!(
            validate_query(&"a".repeat(MAX_QUERY_CHARS + 1)),
            Err(GqlGuardError::TooLong)
        );
        assert_eq!(
            validate_query("{ a(x: \"oops) }"),
            Err(GqlGuardError::Unterminated)
        );
    }
}
