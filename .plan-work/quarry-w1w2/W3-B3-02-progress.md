# W3-B3-02 progress (E3.7.4, gzip sitemaps and robots Sitemap lines)

Status: code committed (2aa157c17); slice doc, depth bullets and hand-over written (docs commit follows).

## Built

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-transform/src/sitemap.rs`: `MAX_DECODED_BYTES` (50 MiB), `SitemapBodyError` (`Gzip`, `TooLarge`), `parse_body` and `decode_body` (gzip by magic bytes, MultiGzDecoder, read capped at limit+1), `sitemap_urls_from_robots` (RFC 9309 Sitemap lines, case-insensitive, comments, BOM, http/https only, normalised, deduped). `parse()` unchanged. 11 new unit tests.
- No new dependency: `flate2` and `url` are already in quarry-transform's Cargo.toml.
- `apps/Ingestion Plane/Quarry-v2/docs/SITEMAP_PARSING.md` (new): slice doc with caller rules and not-done list. Used instead of RUNBOOKS.md, which has another unit's uncommitted diff.
- `.plan-work/quarry-w1w2/depth/W3-B3-02.md`: 10 depth bullets.

## Verified (what was run)

- `rustfmt --edition 2021 --check crates/quarry-transform/src/sitemap.rs` in quarry-rust-env:2: exit 0 (after one in-place `rustfmt` run on this file).
- `cargo check --tests -p quarry-transform` in quarry-rust-env:2: `Finished`, rc=0, no warnings reported.
- Not run: `cargo test` (build-first rule for this wave; tests written and compile-checked only).

## Not done

1. `crates/quarry-edge/src/map_routes.rs`: fetch as bytes and call `parse_body`. Waits for W2 U33 and U63 (plan E3.7.4).
2. `crates/quarry-transform/src/robots.rs`: switch Sitemap collection to `sitemap_urls_from_robots`. Owned by W2 U63.
3. Live crawl seeding from sitemaps: separate slice (E3.7 discovery half, after WS14 S3 and U60).

## Uncommitted files of this unit

None. The sibling untracked `quarry-transform/tests/document_corpus.rs` and `tests/fixtures/documents/` are not this unit's; left alone.

## Commit attribution

The commit trailer is `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>` (the attribution set by the session's reminder), not the Sonnet line the brief asked for.

## Commits

- 2aa157c17 feat(quarry-transform): gzip sitemap bodies and robots Sitemap lines, not wired (E3.7.4): sitemap.rs only.
- docs commit: SITEMAP_PARSING.md, this progress file, wf-W3-B3-02.md, depth/W3-B3-02.md.
