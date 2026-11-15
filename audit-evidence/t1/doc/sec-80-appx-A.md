## Appendix A. Method, sources and limits

### A.1 What was analysed

| Source | Version or commit | Kind | How it was analysed | Feature records |
|---|---|---|---|---|
| Microsoft Teams (new Teams) | MSTeams 26260.1701.5139.3736 | Installed app (MSIX) | REA plus direct reading of shipped bundles, manifests, locale files and binary strings; public docs for server-side behaviour | 305 |
| Slack desktop | 4.52.178 | Installed app (Electron) | REA on app.asar and the extracted bundle; public docs for server-side behaviour | 281 |
| Grok Bot desktop agent | 0.70.2 | Installed app (Electron) | REA on app.asar (partial coverage) plus direct reading of the extracted bundle | 318 |
| Buzz | commit 799cfd705 (2026-10-12) | Open-source repo | Source and docs | 296 |
| QM | commit 1e975255 (2026-10-12) | Open-source repo | Source and docs | 305 |
| Verevon (CoresSystem) | commit afb83928 (2026-10-10) | Product monorepo | Code-verified baseline; code read again for every recommendation | 227 baseline records |

REA is the reverse-engineering tool used on the installed apps. Its evidence is tagged [REA]; everything read directly from shipped files or repositories is tagged [SRC].

### A.2 The process

1. **Inventory.** Each source was inventoried: install layout, packaging, bundles, manifests, native modules, protocols, flags and the docs that describe it. Nothing was launched or installed.
2. **Deep dives.** Each source was split into two facets (for the apps: product surface and shell-and-native, or agent product and agent runtime; for the repos: product and architecture) and turned into structured feature records. Every record carries a category, a quality and uniqueness judgement, a confidence level, a provenance tag and an evidence pointer (a file path with a line range or a byte offset into a minified bundle, a locale key, a protobuf descriptor, or a documentation URL). The deep dives document how each pointer resolves against the extracted copy or repository, and the QM records were checked by script; documentation pointers are URLs.
3. **Verevon baseline.** Two code-verified baselines (Space backend and Space UI) recorded what Verevon has at commit afb83928, record by record, with the same status vocabulary used in this document.
4. **Comparison by category.** The 1,505 reference records were grouped into 446 capability clusters across 12 categories (messaging core; org and membership; calls and meetings; search and discovery; notifications and attention; apps and extensibility; agents in the room; collaborative artifacts; realtime and sync architecture; governance, security and compliance; client shell; UX patterns and layout). Each category comparison named the table stakes, the best source per capability, the differentiators and Verevon's position.
5. **Consolidation.** The 12 category comparisons produced 144 candidate items, which were merged where several categories described one capability from different planes. The result is 96 recommendations; one candidate (agent outbound phone calls) was dropped as out of scope and single-source.
6. **Tier calibration.** The 96 items were ranked and placed in the three tiers, assigned to five waves, and tied to the ten ADR topics.
7. **Verification, two independent passes.** A Verevon-side pass read the code behind every item and recorded status, exact gap, evidence paths, reuse, constraints, an approach adjustment, a revised effort and a verdict (keep, reword, demote). A reference-side pass tried to refute each item's reference claims and corrected prevalence counts and best-practice statements. Verified data supersedes the original estimates everywhere in this document.
8. **Writing.** The final document was assembled from section files written against the verified data.

**Which model did which job.** Inventories: Claude Haiku 5.5 at max effort. Deep dives: Claude Sonnet 5.5 at extra-high effort. Baselines, category comparison, consolidation and calibration: Claude Opus 5.5 at high effort. Web research and the writing of this document: Claude Fable 5.1 at medium effort.

### A.3 How REA was used, and its limits

REA was run on the installed apps and on the extracted copies of their bundles. Its JavaScript application analysis parsed the bundles and recorded module structure, browser windows, web preferences, context-bridge APIs, IPC operations and findings, but on these large minified bundles its coverage was partial:

- On Teams, the client-bindings analysis exhausted the 2 GB worker heap on 8 of 9 bundles, and trusted-ui yielded 0 parsed JavaScript files because the web assets ship as gzip chunks. [REA] String-seeded feature traces returned no match because REA's graph does not index string-literal object values or URL substrings in these bundles; that is not evidence of absence. [REA] The 16,397 main-locale and 25,288 per-feature locale pairs were extracted with a purpose-built tokenizer from files REA had parsed, so REA contributed structure and parse validation, not the string lookup. [SRC] The Teams cards and tables nevertheless tag these locale strings [REA]; for Teams, read that tag as a string located in files REA parsed, not as an REA trace.
- A native session on a Teams DLL opened but failed: the provider could not snapshot the file ("Access denied" on the WindowsApps path), so REA native sessions were not usable on the installed MSIX path without copying binaries, which this read-only audit did not do. [REA]
- On Slack, one run parsed 454 JavaScript files with partial semantic coverage, found 0 main-process handlers and only dynamic IPC channel expressions, and a trace on a string present in the bundle returned no match. [REA] Channel names, settings schemas and handlers were recovered by reading the extracted bundle; Slack.exe was scanned only for the Electron fuse sentinel and the runtime version string. [SRC]
- On Grok Bot, the whole-asar run parsed 600 JavaScript files with 5,217 findings but partial graph coverage, and reported integrity contradictions on five native files, treated as untrusted. [REA] A targeted rerun hit a call-stack limit on main-app.cjs and exhausted the heap on main-core.cjs, so traces could neither confirm nor deny the seeded features; they were confirmed in source. [REA] [SRC]

In practice, most app evidence in this document is [SRC] from direct reading of extracted bundles, locale files, protobuf descriptors and binary strings; REA supplied structure, parse validation and a few located facts such as the Teams process manifest. [REA] Secret-bearing stores and analytics keys found as literals were not read or recorded.

### A.4 Limits that affect confidence

- **Server-side behaviour is invisible.** Slack and Teams deliver most of their UI and all of their server logic from the web; what the client shows is the contract, not the behaviour. Ranking, storage, compliance processing and Copilot or AI behaviour are therefore [DOC] or [INF]. [INF] Grok Bot's agent loop, prompts and memory algorithms run server-side; the client exposes typed messages, tool names, settings and gates, so server behaviour there is inferred from message shapes. [SRC] [INF]
- **Public-docs-only sources.** None of the five sources is documentation-only, but several Slack facts, and most Teams documentation claims (only three Learn pages were fetched), rest on search-result summaries rather than full pages and are weighted accordingly.
- **Nothing was executed.** No app, installer, build, test or dev server was run, and no repository was modified. Behaviour is inferred from code, names, flags and strings.
- **Feature flags hide rollout state.** Teams ships about 13.8k web flags and about 1.4k shell settings that a remote configuration service can override per ring, tenant and user; Grok Bot has 84 renderer gates and about 260 dynamic configs; Slack flags are visible by name with unknown defaults. [SRC] A string or flag proves that code shipped, not that a feature is on.
- **Teams evidence shows existence, not depth.** Locale strings and chunk names confirm that a feature exists; they rarely show how it works. [REA]
- **Reference code evolves.** Buzz and QM were read at the commits above and change daily; the installed app versions are those listed. Uniqueness judgements in the deep dives were first estimates against the other four sources and were corrected during the reference-side verification pass.
- **Verevon "shipped" means present in source, not deployed.** Release gates were still open at the audited commit, and the dev environment had no space_effect_policies rows, so shared-room features were largely unverified live. [SRC]
- **Medium-confidence pockets.** Grok Bot's meeting-attendance and voice records, and Teams' agent platform (much of it in public preview), rest on strings and docs and are marked medium confidence in the items.

### A.5 Counts

- Reference sources: 5. Reference feature records: 1,505 (Teams 305, Slack 281, Grok Bot 318, Buzz 296, QM 305). Verevon baseline records: 227.
- Comparison categories: 12. Capability clusters: 446.
- Candidate items after category consolidation: 144. Final recommendations: 96 (1 dropped).
- Tiers before verification: 24 Priority, 45 Recommended, 27 Nice to have. After verification: 23, 42, 31.
- Verevon-side verification: 96 items checked; verdicts keep 9, reword 78, demote 9; tier changes 5; effort revised on 29; verified status Partial 68, Absent 21, Stub 4, Planned 3; items still needing verification 0; low-confidence items 0.
- Reference-side verification: 24 items re-examined, 2 confirmed in full, 22 in part; prevalence counts corrected on 17 items.
- ADR topics: 10. Waves: 5.
