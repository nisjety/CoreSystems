//! The corpus line format and the embedded corpus. Included by the deterministic
//! gate (`tests/corpus.rs`) and by the live classifier eval in model-gateway, so
//! both read the same rows.
//!
//! One JSON object per line: `id`, `kind` (a label the eval reports in
//! aggregate), `text`, `expect` (`flag` or `clean`) and `needs`. `needs` says
//! who decides the row: `deterministic` means the scanner must get it right, and
//! `semantic` means the scanner must leave it clean, so only the classifier can
//! flag it. Each file holds one combination of `expect` and `needs`.

pub const ATTACKS: &str = include_str!("attacks.jsonl");
pub const ATTACKS_SEMANTIC: &str = include_str!("attacks_semantic.jsonl");
pub const BENIGN: &str = include_str!("benign.jsonl");
pub const BENIGN_SEMANTIC: &str = include_str!("benign_semantic.jsonl");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Flag,
    Clean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Deterministic,
    Semantic,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub expect: Expect,
    pub needs: Needs,
}

pub fn attacks() -> Vec<Row> {
    parse("attacks.jsonl", ATTACKS)
}

pub fn attacks_semantic() -> Vec<Row> {
    parse("attacks_semantic.jsonl", ATTACKS_SEMANTIC)
}

pub fn benign() -> Vec<Row> {
    parse("benign.jsonl", BENIGN)
}

pub fn benign_semantic() -> Vec<Row> {
    parse("benign_semantic.jsonl", BENIGN_SEMANTIC)
}

pub fn all() -> Vec<Row> {
    [attacks(), attacks_semantic(), benign(), benign_semantic()].concat()
}

/// Parses one corpus file. A malformed line panics with its file and line
/// number, never with its text.
pub fn parse(file: &str, jsonl: &str) -> Vec<Row> {
    jsonl
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let number = index + 1;
            let value: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|_| panic!("{file} line {number} is not a JSON object"));
            let field = |name: &str| {
                value[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("{file} line {number} lacks the string field `{name}`"))
                    .to_owned()
            };
            let expect = match field("expect").as_str() {
                "flag" => Expect::Flag,
                "clean" => Expect::Clean,
                _ => panic!("{file} line {number}: expect must be flag or clean"),
            };
            let needs = match field("needs").as_str() {
                "deterministic" => Needs::Deterministic,
                "semantic" => Needs::Semantic,
                _ => panic!("{file} line {number}: needs must be deterministic or semantic"),
            };
            Row {
                id: field("id"),
                kind: field("kind"),
                text: field("text"),
                expect,
                needs,
            }
        })
        .collect()
}
