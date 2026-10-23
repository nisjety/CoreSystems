# W3-B3-03 progress (E3.6-10, document parsing and PII fixture corpus)

Status: BUILT AND COMMITTED (fixtures, loader, slice doc). Compile-checked only; the corpus tests are written and NOT run (build-first rule, end-of-wave run).

## Committed (branch quarryv2-next, local, not pushed)

- d1ab270b9 test(quarry-transform): synthetic Norwegian document corpus and generator (E3.6-10)
  - `crates/quarry-transform/tests/fixtures/documents/no/` NEW: ten documents. Eight PDFs (no-01 text, no-02 text x2, no-03 text, no-04 scan, no-05 text plus scan, no-06 scan with a vector footer, no-07 text with near misses only, no-10 text x2), two DOCX (no-08 paragraphs, no-09 paragraphs and a table).
  - `crates/quarry-transform/tests/fixtures/documents/generate.py` NEW: writes the files and corpus.json; checks every ground-truth value (mod-11, IBAN mod-97, Luhn, phone forms) before writing.
  - `crates/quarry-transform/tests/fixtures/documents/corpus.json` NEW: schema `quarry-document-corpus/1`; per document size, sha256, PDF page kinds, PII truth, near misses, OCR truth.
  - `crates/quarry-transform/tests/fixtures/documents/.gitattributes` NEW: `*.pdf binary`, `*.docx binary`.
- d1fdd8ca3 test(quarry-transform): loader and corpus_* tests for the document corpus (E3.6-10)
  - `crates/quarry-transform/tests/document_corpus.rs` NEW: 7 tests named `corpus_*` (manifest, sha256 and size, page classification, text layers hold the truth, validators accept the truth, every PII kind covered, OCR truth exactly on the scanned PDFs).
- Docs commit: `docs/DOCUMENT_CORPUS.md` NEW (slice doc, used instead of RUNBOOKS.md because RUNBOOKS.md carries another unit's uncommitted hunks), this progress file, the hand-over, and `depth/W3-B3-03.md`.

## Verified (what was actually run)

- `python generate.py` (run in the fixture folder): wrote 10 documents and corpus.json, rc=0. The generator's ground-truth checks passed (no "GROUND TRUTH FAILS" line).
- Python sanity script (not a test suite): every PDF's xref offsets point at `N 0 obj`; both DOCX are valid zips with well-formed `word/document.xml`; the manifest's size and sha256 match each file.
- Visual check of one rendered scan (no-06) in the scratchpad preview: the text reads, including æøå.
- `cargo check --tests -p quarry-transform` through qtest.sh, twice: rc=0, no warnings (before and after the rustfmt pass). `cargo check --test document_corpus -p quarry-transform`: Finished, rc=0.
- `rustfmt --edition 2021 --check document_corpus.rs` natively on the host: rc=0 after formatting in place.
- Not run: `cargo test` (any of the corpus_* tests, or the existing pii_golden tests). Not run: anything against a stack.

## Not done in this unit (by plan or by scope)

- Running the corpus tests (end-of-wave step).
- A test that runs `pii::scan` against the recorded truth and near misses. The estimate does not name it, and it is the measuring step that depends on the fail-closed decision (E3.6-4); see depth bullet "Nothing runs pii::scan over the corpus".
- No real Norwegian documents (the set is synthetic; precision and recall are not measured). Needs owner-approved samples.
- No CID/ToUnicode fonts, encrypted PDFs, Word-saved or scanner-produced files, OCR runs, English set, Office formats beyond DOCX (Office hybrid decision open, E3.6-6).

## Uncommitted files of this unit

None. The sibling units' uncommitted hunks (RUNBOOKS.md, sitemap.rs and others) were not touched and not staged by this unit. Note: when this unit's commits ran, the index already had two staged files of another unit (`git diff --cached` showed 2 files, 257 insertions and 4 deletions before the first commit). qcommit.sh commits only the paths it is given, so those stayed staged and were not committed here.

## Next (for whoever continues)

- End of wave: `cargo test -p quarry-transform corpus_` through qtest. If a near-miss expectation or a page kind is wrong, fix the generator's ground truth (not corpus.json by hand) and regenerate.
- A scanner-against-truth test (pii::scan over no-01..no-10, asserting the PII truth is found and the near misses are not) once E3.6-4 is decided.
- Depth bullets: `.plan-work/quarry-w1w2/depth/W3-B3-03.md`. Hand-over: `.plan-work/quarry-w1w2/wf-W3-B3-03.md`.
