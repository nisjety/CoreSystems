# D2-A-robots-rfc progress (decision 3.2 ROBOTS-RFC, list A item 3)

Unit: close three parser gaps in the `quarry-transform` robots parser, each with a unit test:
1. strip a leading UTF-8 BOM;
2. a Crawl-delay line does not end the user-agent list;
3. an empty `Allow:` line ends the user-agent list like any rule.

Keep e5e14bb6c (a blank line does not split a group). Percent-encoding (D2-184) is not touched.

Worktree: `C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`, local commits only, nothing pushed.
Quarry workspace: `apps/Ingestion Plane/Quarry-v2`. File: `crates/quarry-transform/src/robots.rs` (CRLF, kept).
Scratch: `C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/D2-A-robots-rfc/` (run logs `red-*.log`, `green-*.log`, `final-verify.log`).
HEAD when started: `948d55f3e`. Other units commit on the same branch; my paths were clean at start.
No continuation file existed at start (checked `wf-D2-A*` and `D2-A-robots*`; `D2-A-openapi-null*` belongs to another unit and was not touched).

## Status

All three approved gaps are fixed, each with a red-then-green test. A corpus test and one docs line are added. Verification is green. Two items need an owner decision before the X-10 soak (see Open items).

## Commits (oldest first, branch `quarryv2-next`)

- `c7bf539d4` fix(quarry-transform): robots parser strips a leading UTF-8 BOM
- `ade9c9d98` fix(quarry-transform): a Crawl-delay line does not end the robots user-agent list
- `eddc51302` fix(quarry-transform): an empty Allow line ends the robots user-agent list
- `375172656` test(quarry-transform): robots corpus test over real file shapes
- `837596154` style(quarry-transform): format the robots corpus test as rustfmt does
- `31afcf1bc` docs(crawl-scope): one line on the robots parser rules
- this notes commit: docs(plan-work): D2-A-robots-rfc progress; W2-U63 blank-line note withdrawn

## Spec check (RFC 9309, fetched 2026-10-28 from rfc-editor rfc9309.txt)

- Section 2.2 grammar: `NL = %x0D / %x0A / %x0D.0A`, so a CR-only line end is valid. `str::lines()` splits on LF (and CRLF) only. This is a fourth gap, not one of the three approved; see Open items.
- Section 2.2.4: "Parsing of other records MUST NOT interfere with the parsing of explicitly defined records ... a "Sitemaps" record MUST NOT terminate a group." This grounds gap 2.
- Section 2.2 grammar: `rule` includes `empty-pattern`, so an empty `Allow:` is a rule line. This grounds gap 3.
- The RFC says nothing about a byte order mark. Gap 1 rests on `str::trim` not removing U+FEFF, not on an RFC sentence. The sitemap scanner (`sitemap.rs`) already strips the BOM; the robots parser did not.

## Changes made

- `parse()`: `let input = input.strip_prefix('\u{feff}').unwrap_or(input);` (gap 1).
- `parse()`, `crawl-delay` arm: no longer sets `after_agents` (gap 2). The top comment of `parse()` now says other records do not end the list.
- `parse()`, `allow` arm: sets `after_agents` for an empty value too; an empty pattern is still not pushed (gap 3).
- Test `crawl_delay_any_takes_the_longest_and_ignores_garbage`: its input now has an empty `Disallow:` line between its two groups. Reason: the old input depended on Crawl-delay ending the list, which the decision reverses. The expectation (`Some(5.0)`) is unchanged. Named in commit `ade9c9d98`.
- New tests: `a_leading_utf8_bom_does_not_hide_the_first_user_agent_line`, `a_crawl_delay_line_does_not_end_the_user_agent_list`, `an_empty_allow_line_ends_the_user_agent_list`, `real_file_shapes_decide_as_the_rfc_grammar_reads` (12 corpus rows: BOM, CRLF, comments, Sitemap first, Crawl-delay before the rules, blank lines between user-agent lines, empty Allow, token case; plus a Sitemap and a Crawl-delay check).
- `docs/CRAWL_SCOPE_ROBOTS.md` section 4: one bullet added.

## Test runs (qtest in the Quarry container unless noted)

Red before each fix (the new test failed on its first assertion):
- BOM: `robots` filter 24 passed, 1 failed (`a_leading_utf8_bom...`, assertion `!with_bom.is_allowed`).
- Crawl-delay: 25 passed, 1 failed (`a_crawl_delay_line...`, assertion `!shared.is_allowed("quarrybot", "/private/x")`).
- Empty Allow: 26 passed, 1 failed (`an_empty_allow_line...`, assertion `r.is_allowed("quarrybot", "/x")`).

Green after each fix:
- BOM: `cargo test -p quarry-transform --lib` 202 passed, 0 failed.
- Crawl-delay: `--lib robots` 26 passed, 0 failed.
- Empty Allow: `--lib robots` 27 passed, 0 failed.
- Corpus: `--lib robots` 28 passed, 0 failed.
- After the rustfmt-only change (`837596154`): `--lib robots` 28 passed, 0 failed.

Final verification (one container run, code as of `375172656`; the later commit `837596154` only reformats the corpus test and was re-run on its own as above):
- `cargo test -p quarry-transform --lib`: 205 passed, 0 failed.
- `cargo test -p quarry-runtime --lib robots`: 41 passed, 0 failed.
- `cargo test -p quarry-runtime --test robots_gate_ssrf`: 3 passed, 0 failed.
- `cargo test -p quarry-edge --test crawl_scope_robots`: 15 passed, 0 failed.
- `cargo test -p quarry-edge --test crawl_limits`: 5 passed, 0 failed.
- `cargo test -p quarry-edge --lib robots`: 1 passed, 0 failed (630 filtered out).
- `cargo check --workspace --all-targets`: Finished, exit 0 (1m 02s).
- rustfmt (host rustfmt 1.9.0, edition 2021, `--check`) on robots.rs: 2 hunks, both already in HEAD `948d55f3e` (the `out.groups` chain in the user-agent arm of `parse()`, and the `fold` call in `crawl_delay_any`). Left alone; no new hunks.

## Decisions taken inside the unit

- The crawl_delay_any test input was changed (see Changes made). The expectation was not changed.
- Commit trailer: every commit ends `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`, the model I actually am, per the session attribution reminder. The unit text asked for `Claude Opus 5.5 ... whatever model you are`. That is a conflict: I did not write another model's name. The owner should say which line the repository carries (the same question the earlier D-B1 and W2-U64 notes raised).
- No separate `depth/D2-A-robots-rfc.md` or `wf-D2-A-robots-rfc.md` was written: the subagent rule forbids findings reports. The open items are in this file and in the final JSON, as the D-B1 unit did.
- W2-U63 withdrawal: the decision asks for it in "the ledger". The W2-U63 decision record is `.plan-work/quarry-w1w2/W2-U63-progress.md` ("a named empty group allows everything", line 18). It now has a dated addendum saying the note is withdrawn for consecutive user-agent lines and still holds where a rule line separates the groups. The plan doc has no matching ledger line, and the plan's "Owner decisions of 2026-10-28" section does not name one, so I did not edit the plan.

## Open items (not fixed)

1. **CR-only line endings are read as one line; this is a fail-open gap outside the three approved.** RFC 9309 section 2.2 allows CR alone as a line end. `str::lines()` splits on LF and CRLF only, so `User-agent: *\rDisallow: /` becomes one line with token `*\rdisallow: /`, no rules, and everything is allowed. Basis: the code and the RFC grammar, not run. Suggested fix: normalise `\r\n` and lone `\r` to `\n` before `lines()`, with one test. The owner should approve it before the soak, because the soak measures this parser.
2. **A Crawl-delay line now shares a group with the user-agent lines around it (the approved reading, but the owner should see the consequence).** Verified by the corpus row `User-agent: quarrybot / Crawl-delay: 2 / User-agent: * / Disallow: /private/`: quarrybot is blocked from `/private/` (the test passes). So a real file that gives each bot its own Crawl-delay and then a `*` group with a Disallow now makes the named bot inherit that Disallow; before this change the named bot was allowed. Also by reading the code (not run): a group with two Crawl-delay lines keeps the last value (`crawl_delay_s = Some(n)` overwrites). Options for the owner: keep the RFC 2.2.4 reading as built; or ask for a different rule for Crawl-delay lines that sit between user-agent lines; or take the maximum when a group has two values.
3. Percent-encoding normalisation (D2-184): not touched, as instructed.
4. A user-agent token with a version (`QuarryBot/1.0`) does not match. RFC 9309 product tokens contain no `/`, so such a line is not a valid group. Minor; not changed, not in the corpus.

## Left uncommitted in the tree (not mine)

Other units' edits in the same tree (quarry-edge `lib.rs`, `main.rs`, `fixture_site.rs`, quarry-runtime `step_receipts.rs`, `rb.patch`, `.plan-work/next-slices.md`, and the D2-A-openapi-null notes). Not touched.

## Next

Nothing left in scope except the two owner decisions in Open items (items 1 and 2). They change what the X-10 soak measures.
