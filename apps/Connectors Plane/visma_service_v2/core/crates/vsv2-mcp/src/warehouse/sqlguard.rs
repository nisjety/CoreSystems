//! Static read-only guard for ad-hoc warehouse SQL. Defense in depth: BigQuery's own dry-run
//! (`statementType == SELECT`) is the authority; this rejects the obvious before spending a call.

use thiserror::Error;

pub const MAX_SQL_CHARS: usize = 20_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GuardError {
    #[error("SQL is empty")]
    Empty,
    #[error("SQL is longer than {MAX_SQL_CHARS} characters")]
    TooLong,
    #[error("only a single statement is allowed (found a `;` before the end)")]
    MultipleStatements,
    #[error("only SELECT / WITH queries are allowed, got `{0}`")]
    NotSelect(String),
    #[error("`{0}` is not allowed in a read-only query (quote it with backticks if it is a column name)")]
    Forbidden(String),
    #[error("unterminated string, quoted name or comment")]
    Unterminated,
}

const FORBIDDEN: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "DROP",
    "CREATE",
    "ALTER",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
    "EXPORT",
    "LOAD",
    "CALL",
    "EXECUTE",
    "ASSERT",
    "DECLARE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "RAISE",
    "EXTERNAL_QUERY",
];

/// Replace the *contents* of strings, quoted identifiers and comments with spaces, keeping
/// everything else. `r'..'`/`R".."` raw strings have no backslash escapes.
fn skeleton(sql: &str) -> Result<String, GuardError> {
    let c: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        // line comments: `#` and `--`
        if ch == '#' || (ch == '-' && c.get(i + 1) == Some(&'-')) {
            while i < c.len() && c[i] != '\n' && c[i] != '\r' {
                i += 1;
            }
            out.push(' ');
            continue;
        }
        // block comment
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            loop {
                if i + 1 >= c.len() {
                    return Err(GuardError::Unterminated);
                }
                if c[i] == '*' && c[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            out.push(' ');
            continue;
        }
        if ch == '\'' || ch == '"' || ch == '`' {
            let raw = is_raw_prefix(&c, i);
            let triple = ch != '`' && c.get(i + 1) == Some(&ch) && c.get(i + 2) == Some(&ch);
            let quote_len = if triple { 3 } else { 1 };
            i += quote_len;
            loop {
                if i >= c.len() {
                    return Err(GuardError::Unterminated);
                }
                if !raw && c[i] == '\\' {
                    i += 2;
                    continue;
                }
                let closes = c[i] == ch
                    && (!triple || (c.get(i + 1) == Some(&ch) && c.get(i + 2) == Some(&ch)));
                if closes {
                    i += quote_len;
                    break;
                }
                i += 1;
            }
            out.push(' ');
            continue;
        }
        out.push(ch);
        i += 1;
    }
    Ok(out)
}

/// `r'..'`, `R"..""`, `rb'..'`, `br'..'`: the quote is preceded by a prefix containing r/R that
/// is not the tail of a longer identifier.
fn is_raw_prefix(c: &[char], quote_at: usize) -> bool {
    let mut j = quote_at;
    let mut prefix = String::new();
    while j > 0 && prefix.len() < 2 && matches!(c[j - 1], 'r' | 'R' | 'b' | 'B') {
        prefix.push(c[j - 1]);
        j -= 1;
    }
    let boundary = j == 0 || !(c[j - 1].is_alphanumeric() || c[j - 1] == '_');
    boundary && prefix.chars().any(|p| matches!(p, 'r' | 'R'))
}

/// `ML.<fn>(` / `AI.<fn>(` can call remote models (cost and data leaving BigQuery). Matches the call
/// shape only, so a column or alias merely named `ml` or `ai` is fine.
fn remote_model_call(body: &str) -> Option<String> {
    let c: Vec<char> = body.chars().collect();
    let skip_ws = |mut i: usize| {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        i
    };
    let ident_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut i = 0;
    while i < c.len() {
        if !ident_char(c[i]) || (i > 0 && (ident_char(c[i - 1]) || c[i - 1] == '.')) {
            i += 1;
            continue;
        }
        let start = i;
        while i < c.len() && ident_char(c[i]) {
            i += 1;
        }
        let word: String = c[start..i].iter().collect();
        if !(word.eq_ignore_ascii_case("ML") || word.eq_ignore_ascii_case("AI")) {
            continue;
        }
        let j = skip_ws(i);
        if c.get(j) != Some(&'.') {
            continue;
        }
        let k = skip_ws(j + 1);
        let mut m = k;
        while m < c.len() && ident_char(c[m]) {
            m += 1;
        }
        if m > k && c.get(skip_ws(m)) == Some(&'(') {
            let func: String = c[k..m].iter().collect();
            return Some(format!(
                "{}.{}",
                word.to_ascii_uppercase(),
                func.to_ascii_uppercase()
            ));
        }
    }
    None
}

fn words(sk: &str) -> impl Iterator<Item = &str> {
    sk.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|w| !w.is_empty())
}

/// Returns the SQL trimmed of whitespace and one optional trailing `;`.
pub fn validate_sql(sql: &str) -> Result<String, GuardError> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(GuardError::Empty);
    }
    if sql.chars().count() > MAX_SQL_CHARS {
        return Err(GuardError::TooLong);
    }
    let sk = skeleton(sql)?;
    let body = sk.trim_end().trim_end_matches(';').trim_end();
    if body.contains(';') {
        return Err(GuardError::MultipleStatements);
    }
    let first = words(body).next().unwrap_or("").to_ascii_uppercase();
    let starts_paren = body.trim_start().starts_with('(');
    if !(starts_paren || first == "SELECT" || first == "WITH") {
        return Err(GuardError::NotSelect(first));
    }
    for w in words(body) {
        let up = w.to_ascii_uppercase();
        if FORBIDDEN.contains(&up.as_str()) {
            return Err(GuardError::Forbidden(up));
        }
    }
    if let Some(f) = remote_model_call(body) {
        return Err(GuardError::Forbidden(f));
    }
    let cut = sql.trim_end().trim_end_matches(';').trim_end();
    Ok(cut.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) {
        validate_sql(s).unwrap_or_else(|e| panic!("should pass: {s:?}: {e}"));
    }
    fn bad(s: &str) -> GuardError {
        validate_sql(s).expect_err(&format!("should be rejected: {s:?}"))
    }

    #[test]
    fn accepts_ordinary_read_queries() {
        ok("SELECT 1");
        ok("  select a, b from `p.d.t` where x = 'y' limit 5;  ");
        ok("WITH x AS (SELECT 1 AS a) SELECT * FROM x");
        ok("(SELECT 1) UNION ALL (SELECT 2)");
        ok("SELECT updated_at, created_by, deleted_at FROM t"); // identifiers merely containing keywords
        ok("SELECT 'DROP TABLE x; DELETE FROM y' AS s"); // keywords inside strings
        ok("SELECT 1 -- ; DROP TABLE x\n");
        ok("SELECT 1 /* ; DELETE FROM x */");
        ok("SELECT `delete` FROM t"); // quoted identifier
    }

    #[test]
    fn rejects_dml_ddl_and_scripts() {
        for s in [
            "DELETE FROM t WHERE TRUE",
            "UPDATE t SET a=1 WHERE TRUE",
            "INSERT INTO t VALUES (1)",
            "MERGE t USING s ON TRUE WHEN MATCHED THEN DELETE",
            "DROP TABLE t",
            "CREATE TABLE t AS SELECT 1",
            "TRUNCATE TABLE t",
            "EXPORT DATA OPTIONS(uri='gs://x') AS SELECT 1",
            "CALL proc()",
            "BEGIN SELECT 1; END",
            "DECLARE x INT64",
        ] {
            bad(s);
        }
    }

    #[test]
    fn rejects_stacked_statements_and_forbidden_words_hidden_after_a_select() {
        assert_eq!(
            bad("SELECT 1; DROP TABLE t"),
            GuardError::MultipleStatements
        );
        assert_eq!(bad("SELECT 1; SELECT 2"), GuardError::MultipleStatements);
        assert!(matches!(
            bad("SELECT * FROM t WHERE EXISTS (DELETE FROM x)"),
            GuardError::Forbidden(_)
        ));
        assert!(matches!(
            bad("SELECT * FROM EXTERNAL_QUERY('conn', 'select 1')"),
            GuardError::Forbidden(w) if w == "EXTERNAL_QUERY"
        ));
    }

    #[test]
    fn raw_strings_do_not_let_a_backslash_hide_a_second_statement() {
        // BigQuery: r'\' is a complete raw string, so `; DROP ...` is real code.
        assert!(matches!(
            bad(r#"SELECT r'\' ; DROP TABLE x; --'"#),
            GuardError::MultipleStatements | GuardError::Forbidden(_)
        ));
        // A normal string with an escaped quote is still one string.
        ok(r#"SELECT 'it\'s; DROP' AS s"#);
        ok("SELECT '''multi ; DROP\nline''' AS s");
    }

    #[test]
    fn rejects_non_select_starts_empty_long_and_unterminated() {
        assert!(matches!(bad("SHOW TABLES"), GuardError::NotSelect(_)));
        assert_eq!(bad("   "), GuardError::Empty);
        assert_eq!(
            bad(&format!("SELECT '{}'", "a".repeat(MAX_SQL_CHARS))),
            GuardError::TooLong
        );
        assert_eq!(bad("SELECT 'abc"), GuardError::Unterminated);
        assert_eq!(bad("SELECT 1 /* never closed"), GuardError::Unterminated);
    }

    #[test]
    fn carriage_return_ends_a_line_comment_like_bigquery_does() {
        // BigQuery treats \r as a line end, so everything after it is live SQL.
        assert!(matches!(
            bad("SELECT 1 -- x\r; DROP TABLE t"),
            GuardError::MultipleStatements
        ));
        assert!(matches!(
            bad("SELECT 1 # x\rDELETE FROM t"),
            GuardError::Forbidden(_)
        ));
    }

    #[test]
    fn remote_model_function_calls_are_refused_but_plain_names_are_not() {
        for s in [
            "SELECT ML.GENERATE_TEXT(MODEL m, (SELECT 'x' AS prompt)) FROM t",
            "SELECT AI . GENERATE ( 'x' )",
            "SELECT ml.predict(MODEL m, TABLE t)",
        ] {
            assert!(
                matches!(bad(s), GuardError::Forbidden(f) if f.starts_with("ML.") || f.starts_with("AI.")),
                "{s}"
            );
        }
        ok("SELECT ml, ai FROM t");
        ok("SELECT t.ai.x, ml_score FROM t AS t");
        ok("SELECT html.x, domain.y FROM t");
        ok("SELECT 'ML.PREDICT(' AS s");
    }

    #[test]
    fn returns_the_sql_without_the_trailing_semicolon() {
        assert_eq!(validate_sql("  SELECT 1 ;  ").unwrap(), "SELECT 1");
    }
}
