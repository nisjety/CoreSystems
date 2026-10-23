# W3-B1-03 progress: E3.6-2 parser-bomb tests and E3.6-3 PDF text-or-scan classifier

Date: 2026-10-23. Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b, branch quarryv2-next.

## Committed

- ebe43222f feat(quarry-transform): docx budget on declared part sizes, zip-bomb tests (E3.6-2)
  - docx.rs only. Reads each part's declared size from the central directory before inflating. Refuses a word/document.xml that declares more than 32 MiB, and the parts together past MAX_DOCX_DECLARED_BYTES (256 MiB). Adds three tests (two 150 MiB parts refused "in total"; a declared oversize document.xml refused "declares"; a docx with a 2 MiB media part still extracts). Fixtures built in code.
- ff58b0bdb feat(quarry-transform): per-page text-or-scan classifier for PDF (E3.6-3)
  - pdf.rs: classify_pages, classify_page (pure decision), PageKind, PageClassification, MIN_TEXT_CHARS = 20, resources_of (bounded walk, 32 links), page_has_image, page_text_chars. Tests (decision table, text vs scan in order, footer over scan, blank and short text, inherited resources, cyclic page tree, serde spelling, classification behind the inflate budget).
  - docs/DOCUMENT_PARSE_LIMITS.md (new): the DOCX budget table and the classifier rule. Used instead of a RUNBOOKS section because RUNBOOKS.md carries another unit's uncommitted hunks.

## Verified (compile-check only; no test suite run, per the W3 build-first rule)

- `cargo check --tests -p quarry-transform` through qtest.sh: rc=0, no warnings, run before the commit ff58b0bdb (the first run compiled; a second run after the final `kids` annotation edit also gave rc=0, "Finished" in 2.77 s, no warnings).
- Test counts written (not run): docx.rs 3 new tests; pdf.rs 10 new tests (decision table, two-page order, footer over scan, blank and short text, inherited resources, cyclic tree, serde spelling, classification behind budget; plus helpers build_pdf and PageSpec).

## Not done in this unit

- Routing: no pipeline call, no mp_client request, no model_plane_gate row. The classifier is dark.
- RUNBOOKS section: replaced by docs/DOCUMENT_PARSE_LIMITS.md (see above).
- Tests not run: `cargo test -p quarry-transform` is the W3 end-of-wave step.

## Files of other units (not committed by this unit)

- crates/quarry-transform/src/lib.rs: one uncommitted `pub mod canonical_url;` line (W3-B1-04's), with src/canonical_url.rs and tests/canonical_url.rs untracked. Left alone.
- docs/RUNBOOKS.md: uncommitted hunks from another unit (25 insertions, 18 deletions). Left alone.
- .plan-work/quarry-w1w2/W2-U09-progress.md and other W2 files: left alone.

## Next (for whoever continues)

- E3.6-10 fixture corpus (B3) supplies the real Norwegian PDFs and docx files this classifier and the budgets need. Until then, MIN_TEXT_CHARS and the docx total cap are judgment calls, listed in the depth file.
- The routing slice (E3.6-4 and E3.6-6, both gated on decisions) reads PageKind; it should take one parse instead of calling load_doc again.
