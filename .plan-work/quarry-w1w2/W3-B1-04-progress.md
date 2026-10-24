# W3-B1-04 progress (E3.7.6 canonical URL helper)

Build-first rule: built, tests written, compile-checked only. Nothing run.

Committed (branch quarryv2-next, local, not pushed):
- aa41358ed feat(quarry-transform): canonical URL helper and redirect reconciliation (WS14 E3.7.6). Files: src/canonical_url.rs (new), tests/canonical_url.rs (new).
- aa23352a3 feat(quarry-transform): expose the canonical_url module (WS14 E3.7.6). One lib.rs line, committed from the HEAD blob (qcommit-blobs.sh) so the sibling pii line in the working copy was not committed.
- 1e0070413 docs(quarry-transform): runbook section for the canonical URL helper (WS14 E3.7.6). RUNBOOKS.md committed from the HEAD blob plus my section only.

Verified:
- rustfmt --edition 2021 on the two new files (applied in place), exit 0.
- cargo check --tests -p quarry-transform through qtest: exit 0, no error or warning lines.

Not done:
- Enqueue wiring (enqueue_batch, seeds through the scope filter, redirect seen-marking): waits for W2 U60 (WS14.S3).
- Tests not run (test phase at the end of the wave).

Uncommitted, not mine: quarry-transform src/pii.rs, pii fixtures, docx.rs, pdf.rs, lib.rs `pub mod pii;` line (W3-B1-02 and W3-B1-03). RUNBOOKS.md working-copy reordering (see depth file).

Depth bullets: .plan-work/quarry-w1w2/depth/W3-B1-04.md
