//! HTML → markdown via html2md (Phase-1 implementation; readability-first pipeline tracked in roadmap).
//!
//! Tables are rendered by [`CompactTableHandler`] instead of html2md's own table
//! handler. html2md pads every cell to the widest cell of its column, so one very
//! wide cell made every row of the table as wide (a 1 MB page produced 30.9 MB of
//! markdown), and it collected the rows of nested tables once per nesting level,
//! which grows exponentially with the depth (434 bytes of HTML with ten nested
//! tables produced 770 MB in three minutes). The compact handler writes each cell
//! once, unpadded, and gives every table only its own rows.

use std::collections::HashMap;
use std::sync::LazyLock;

use html2md::{walk, Handle, NodeData, StructuredPrinter, TagHandler, TagHandlerFactory};
use regex::Regex;

pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html_custom(html, &custom_handlers())
}

fn custom_handlers() -> HashMap<String, Box<dyn TagHandlerFactory>> {
    let mut handlers: HashMap<String, Box<dyn TagHandlerFactory>> = HashMap::new();
    handlers.insert("table".to_owned(), Box::new(CompactTableHandler::default));
    handlers
}

/// Renders a `<table>` as a GitHub-style pipe table without column padding.
///
/// The first row is the header and gets the delimiter row (with `:` alignment
/// marks taken from the header cells' `align` attribute). Rows are written with
/// the cells they have: a short row is not filled up to the widest row, because
/// that fill is itself proportional to rows x columns. Only the header row is
/// completed to the column count so the delimiter row matches it.
#[derive(Default)]
pub struct CompactTableHandler;

impl TagHandler for CompactTableHandler {
    fn handle(&mut self, tag: &Handle, printer: &mut StructuredPrinter) {
        let mut rows = Vec::new();
        collect_rows(tag, &mut rows);
        let cells: Vec<Vec<Handle>> = rows.iter().map(|row| cells_of(row)).collect();
        let column_count = cells.iter().map(Vec::len).max().unwrap_or(0);
        if column_count == 0 {
            return;
        }

        let mut table = String::new();
        for (index, row) in cells.iter().enumerate() {
            table.push('|');
            let width = if index == 0 { column_count } else { row.len() };
            for column in 0..width {
                table.push(' ');
                if let Some(cell) = row.get(column) {
                    table.push_str(&cell_text(cell));
                    table.push(' ');
                }
                table.push('|');
            }
            table.push('\n');

            if index == 0 {
                table.push('|');
                for column in 0..column_count {
                    let marker = match row.get(column).map(align_of).as_deref() {
                        Some("left") => " :--- |",
                        Some("center") => " :---: |",
                        Some("right") => " ---: |",
                        _ => " --- |",
                    };
                    table.push_str(marker);
                }
                table.push('\n');
            }
        }

        printer.insert_newline();
        printer.insert_newline();
        printer.append_str(&table);
    }

    fn after_handle(&mut self, _printer: &mut StructuredPrinter) {}

    fn skip_descendants(&self) -> bool {
        true
    }
}

fn tag_name(handle: &Handle) -> Option<String> {
    match handle.data {
        NodeData::Element { ref name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

/// The `<tr>` elements of one table, in document order. The search goes through
/// `thead`/`tbody`/`tfoot` and stops at a `<tr>` (its cells are read by
/// [`cells_of`]) and at a nested `<table>` (that table renders its own rows
/// inside the cell that holds it).
fn collect_rows(handle: &Handle, rows: &mut Vec<Handle>) {
    for child in handle.children.borrow().iter() {
        match tag_name(child).as_deref() {
            Some("tr") => rows.push(child.clone()),
            Some("table") => {}
            _ => collect_rows(child, rows),
        }
    }
}

fn cells_of(row: &Handle) -> Vec<Handle> {
    row.children
        .borrow()
        .iter()
        .filter(|child| matches!(tag_name(child).as_deref(), Some("td" | "th")))
        .cloned()
        .collect()
}

fn align_of(cell: &Handle) -> String {
    match cell.data {
        NodeData::Element { ref attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|attr| &*attr.name.local == "align")
            .map(|attr| attr.value.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

static BLANK_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?m)^ +$").expect("blank line"));
static EXCESS_NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\n{3,}").expect("newlines"));

/// The text of one cell: its content as markdown on a single line (line breaks
/// become `<br/>`, as html2md's own table handler does), trimmed.
fn cell_text(cell: &Handle) -> String {
    let mut printer = StructuredPrinter::default();
    walk(cell, &mut printer, &custom_handlers());
    let text = BLANK_LINE.replace_all(&printer.data, "");
    let text = EXCESS_NEWLINES.replace_all(&text, "\n\n");
    text.trim().replace('\n', "<br/>")
}

#[cfg(test)]
mod table_size_tests {
    use super::html_to_markdown;

    /// D1-026: a table-heavy page with a wide cell in each column, on different rows. html2md
    /// padded every column to its widest cell, so each of the 1,500 rows paid for all three wide
    /// cells (a 1 MB reference page became 30.9 MB of markdown). Compact rows keep the output
    /// close to the input.
    #[test]
    fn wide_cells_in_several_columns_do_not_pad_the_other_rows() {
        let wide = |c: char| c.to_string().repeat(4_000);
        let mut html = String::from("<table><tr><th>a</th><th>b</th><th>c</th></tr>");
        for i in 0..1_500 {
            let a = if i == 100 { wide('x') } else { format!("a{i}") };
            let b = if i == 700 { wide('y') } else { format!("b{i}") };
            let c = if i == 1_300 { wide('z') } else { format!("c{i}") };
            html.push_str(&format!("<tr><td>{a}</td><td>{b}</td><td>{c}</td></tr>"));
        }
        html.push_str("</table>");
        let md = html_to_markdown(&html);
        assert!(
            md.len() <= html.len() * 2,
            "{} bytes of html became {} bytes of markdown",
            html.len(),
            md.len()
        );
        for (cell, row) in [(wide('x'), 100), (wide('y'), 700), (wide('z'), 1_300)] {
            assert!(md.contains(&cell), "the wide cell of row {row} is kept whole");
        }
        for i in [0, 699, 1_499] {
            assert!(md.contains(&format!("| a{i} | b{i} | c{i} |")), "row {i} lost");
        }
    }
}
