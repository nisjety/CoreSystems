# Document parsing and PII fixture corpus (E3.6-10)

Status: fixtures and loader only. Nothing in Quarry reads the corpus at runtime
and no stack is involved. The loader compiles (`cargo check --test
document_corpus -p quarry-transform`); its tests are written and have not been
run (build-first rule, the W3 end-of-wave run is the first run).

The corpus is the test input the later E3.6 slices need: the PDF text-or-scan
classifier (E3.6-3), the DOCX budgets (E3.6-2), the PII validators and scanner
(E3.6-1), and the redaction wiring (E3.6-4, gated). It is the "10-document
Norwegian public-sector fixture set" that the plan's E3.6 risks name, built
before the `on` default is considered.

## Where it lives

- `crates/quarry-transform/tests/fixtures/documents/corpus.json`: the manifest
  and the ground truth (schema `quarry-document-corpus/1`).
- `crates/quarry-transform/tests/fixtures/documents/no/`: the ten documents.
- `crates/quarry-transform/tests/fixtures/documents/generate.py`: writes the
  documents and the manifest. Run it from that folder with `python generate.py`.
  Pillow is needed for the scan rendering.
- `crates/quarry-transform/tests/fixtures/documents/.gitattributes`: PDF and
  DOCX are `binary`, so no line-ending conversion touches them.
- `crates/quarry-transform/tests/document_corpus.rs`: the loader and its tests.

Every file is SYNTHETIC. The names, case numbers and addresses are invented.
Identity numbers pass the mod-11 control and belong to no one. The bank
account and card numbers are published examples. The generator checks every
ground-truth value against its rule (mod-11 for identity numbers, mod-97 for
IBAN, Luhn for cards, the phone forms) before it writes anything.

## The ten documents

| Id | Format | Pages | Shape under test | Personal values (ground truth) |
| --- | --- | --- | --- | --- |
| `no-01-vedtak-bostotte` | PDF | text | decision letter | identity number, email, `+47` phone, Norwegian IBAN |
| `no-02-soknad-barnehageplass` | PDF | text x2 | application form across pages | two identity numbers, national mobile, email, card |
| `no-03-faktura-kort` | PDF | text | invoice | email, IBAN, card. Near misses: org number, KID, a Luhn-failing order number |
| `no-04-skannet-innkalling` | PDF | scan | image with no text layer, OCR truth listed | H-number, `+47` phone |
| `no-05-blandet-notat-og-vedlegg` | PDF | text, scan | one document, two kinds of page | phone, email, identity number, national phone (page 2 via OCR truth) |
| `no-06-skann-kvittering-sidefot` | PDF | scan | vector footer over a scan (footer is under the text threshold) | published card number |
| `no-07-nare-treff` | PDF | text | no personal data at all | none. Case, order and org numbers, UUID, git SHA, postal codes, a date |
| `no-08-vedtak-tilskudd` | DOCX | paragraphs | grant decision | email, identity number, `+47` phone, IBAN |
| `no-09-oppdragsavtale-tabell` | DOCX | paragraphs and a table | D-number, two IBANs (one German published example) | D-number, identity number, IBAN x2, phone |
| `no-10-journalnotat` | PDF | text x2 | identity number written with a space, national phone | identity number, national phone, email |

Scanned pages carry no text layer, so their personal values are checked
against `ocr_truth`, the text a correct OCR run should read.

## Ground truth in `corpus.json`

- `pii`: each value a correct detector must redact, exactly as written.
- `near_miss`: strings that look like personal data and must not be redacted.
- `pages`: the expected kind of each PDF page (`text`, `scanned`), by the rule
  of `pdf::classify_page` (`MIN_TEXT_CHARS` = 20 non-white characters, image
  present means scanned).
- `ocr_truth`: text of the scanned pages, line by line.
- `sha256` and `bytes`: the recorded bytes of each file.

## Tests (named `corpus_*`)

Run with `cargo test -p quarry-transform corpus_` at the end of the wave.

- `corpus_manifest_is_the_norwegian_set_of_ten`: ten Norwegian documents, unique
  ids, a text-only PDF, a scanned-only PDF, a mixed PDF and a DOCX are present.
- `corpus_files_match_their_recorded_size_and_sha256`: size, SHA-256 and the
  file header (`%PDF-` or the zip signature).
- `corpus_pdf_pages_classify_as_recorded`: `pdf::classify_pages` gives the
  recorded kind for every page.
- `corpus_text_layers_hold_every_recorded_personal_value`: every `pii` value and
  every `near_miss` string appears in the text the reader gives, or in
  `ocr_truth` for a scan.
- `corpus_pii_truth_passes_the_validators`: every `pii` value passes its
  `pii::is_*` validator, so the ground truth is not wrong.
- `corpus_covers_every_pii_kind`: each of the five kinds occurs at least once.
- `corpus_scanned_pages_carry_ocr_truth_and_text_pages_do_not`: OCR truth exists
  exactly for the PDFs with a scanned page.

## Regeneration and determinism

The PDF layout, the DOCX parts and the zip timestamps are fixed by the
generator, so the bytes do not change between runs on the same Pillow. The
scan rasters use Pillow's bundled default font (`ImageFont.load_default`). A
different Pillow version can change those pixels, which changes the scan files
and their SHA-256. In that case regenerate, and the manifest is rewritten with
the new hashes in the same step. Do not hand-edit `corpus.json`.

## Not covered (by design, or not yet)

- No real Norwegian documents. The set is synthetic and only proves the
  readers and validators run on the shapes. Precision, recall and the
  thresholds (`MIN_TEXT_CHARS`, the phone and identity-number candidates) are
  not measured here; they need real samples the owner approves.
- No PDF with CID or `ToUnicode` fonts, encrypted PDFs, or real Word or scanner
  output. The lossy `Tj`/`TJ` risk in the plan is therefore not tested.
- No OCR is run. The scans are clean rasters, easier than real scans (skew,
  noise, JPEG artefacts).
- No English documents. English PII cases are in `tests/fixtures/pii/golden.json`.
- No XLSX, PPTX, ODF or EPUB. The Office path is an open decision (E3.6-6).
- Parser-bomb inputs (zip and flate bombs) are built in code in `docx.rs` and
  `pdf.rs` tests, not in this folder.
- The corpus is not wired to any route and the redaction stage (E3.6-4) is not
  built, so the corpus does not test a redacted output yet.
