# Document parse limits and the PDF page classifier

Status: built, dark. Nothing here routes a page anywhere yet. Written under the
W3 build plan, epics E3.6-2 and E3.6-3. Tests are written and compile-checked;
they have not been run.

## DOCX budgets (quarry-transform `docx.rs`)

`extract_text` refuses a DOCX before it inflates any part:

| Limit | Value | Refusal |
|---|---|---|
| File size | 25 MiB | `BadRequest`, "docx exceeds" |
| Entries in the zip | 4096 | `BadRequest`, "docx has N entries" |
| `word/document.xml` declared size | 32 MiB | `BadRequest`, "declares ... exceeds" |
| All parts, declared sizes summed | 256 MiB | `BadRequest`, "parts declare ... in total" |
| `word/document.xml` streamed size | 32 MiB | `BadRequest`, "exceeds" (the real limit) |

Declared sizes come from the zip central directory, so the total check costs no
inflation. A declared size can lie, which is why the streamed cap stays.

Test fixtures are built in code (`docx_with_parts`), not committed as binaries.

## PDF page classifier (quarry-transform `pdf.rs`)

`classify_pages(body)` returns one `PageClassification` per page, in page order:
`page` (1-based), `kind` (`text`, `scanned`, `empty`), `text_chars` (non-white-space
characters the page shows) and `has_image` (the page's resources, or an ancestor's,
name an image XObject).

The decision (`classify_page`):

1. `text_chars >= MIN_TEXT_CHARS` (20): `text`.
2. Otherwise, `has_image`: `scanned`. A page number over a scan is a scan.
3. Otherwise, `text_chars > 0`: `text` (short text with no image).
4. Otherwise: `empty`.

The rule leans to `scanned` when an image is present and the text is short. A
scanned page sent to a layout or OCR route costs a call; a scan read as text
loses its content.

`classify_pages` parses through the same guarded loader as `extract_text`, so the
inflate budget (`INFLATE_BUDGET_BYTES`, `IMAGE_INFLATE_BUDGET_BYTES`) applies first:
a bomb is an error, not a classification. The page tree walk for `Resources` is
bounded (32 parent links), so a cyclic `/Parent` chain terminates.

## Known limits

- Images painted only through a form XObject, or as inline images (`BI`/`EI`), are
  not seen; the page reads as text or empty. Recorded in the W3 depth list.
- The classifier does not route. The Model Plane layout route and the pipeline
  wiring are later slices (E3.6-4 and E3.6-6 territory).
