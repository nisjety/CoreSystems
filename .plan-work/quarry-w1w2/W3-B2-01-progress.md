# W3-B2-01 progress (E3.7.8 pricing function)

Status: built, dark, committed in two steps. Nothing calls the function.

## Committed

- `5135b1a37` feat(runtime): `quarry-runtime/src/job_pricing.rs` (new, pure) and one `pub mod job_pricing;` line in `quarry-runtime/src/lib.rs`. Carries no other unit's hunk (`git diff` on lib.rs showed the one line only).
- `1bad0529a` docs(quarry): `apps/Ingestion Plane/Quarry-v2/docs/JOB_PRICING_E3_7_8.md` (slice doc; RUNBOOKS.md not touched because another unit has uncommitted hunks there).

## Verified (what was run)

- `cargo check -p quarry-runtime --tests` through qtest (quarry-rust-env:2): exit 0, `Finished dev profile`, no errors, no warnings that point at job_pricing.rs.
- Earlier attempts failed on `quarry-transform/src/parse_cache.rs` (`ParseCacheKey` lacks `Debug`), a sibling unit's uncommitted file. Not ours; it passed on the retry, so the file was fixed in the tree.
- Not run: the 12 unit tests in `job_pricing.rs` (build-first rule).

## Scope note

The plan's E3.7 section names no pricing function and no E3.7.8 paragraph. The slice was taken from the W3 estimate's B2 line ("pricing fn, quarry-runtime new module + lib line") plus the X-12 budget rules (estimator rate, ceilings), R-9 and the flat action table. Plan owner should add the slice row.

## Design

- `estimate_job_price(JobPriceInput, JobRates) -> JobPrice`, pure, saturating.
- `pages` = fetch units: pages (batch, crawl), sitemap documents (map), result pages (search hydration).
- Static fetch per unit per attempt; browser time per browser unit per attempt; map never prices browser time.
- Clamps: attempts 0 means 1, above 5 clamps to 5; pages above 5000 clamps; browser units above pages clamp. Every clamp sets `clamped`.
- Rates are placeholders: 1000 micro-USD static (flat `navigate`), 30 micro-USD per browser second (budget.rs), 10 browser seconds per page (unmeasured).

## Not done

- No caller. E3.7.9 (approval gate, needs the crawl-approval decision) and WS15 L1 (job record) wire it later.
- Rates are constants, no env override; ratified numbers (X-12) and billing-core L4 pending.
- No page-count approval threshold (decision, not this slice).
- Tests not run (build-first rule).

## Next

1. When W3 tests phase runs: `cargo test -p quarry-runtime job_pricing`.
2. When X-12 is ratified: replace `JobRates::default` and move the ceilings to env.

Depth bullets: `.plan-work/quarry-w1w2/depth/W3-B2-01.md`
Hand-over: `.plan-work/quarry-w1w2/wf-W3-B2-01.md`
