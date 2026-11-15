## Appendix A. Method, sources and limits

### A.1 What was analysed

| Source | Version or commit | Kind | How it was analysed | Feature records |
|---|---|---|---|---|
| Codex desktop app (in-app browser, computer use) | 26.1007.2314.0 | Installed app | REA on the app.asar and native binaries; the pre-extracted bundle and plugin scripts were beautified and read directly | 261 |
| Aside browser | 1.0.928.2 | Installed app | REA native, PE and string analysis; no JavaScript bundle for the daemon, so the embedded payload and extension sources were read directly | 187 |
| Firecrawl | commit 11e9dc5e2 (2026-10-12) | Open-source repository at C:/dev/firecrawl | Source and in-repo docs | 226 |
| Apify platform | public docs, October 2026 | Public documentation | Public web docs only; every Apify claim is DOC | 112 |
| Verevon (CoresSystem) | commit afb83928 (2026-10-10) | Git monorepo at C:/dev/CoresSystem | Code read as ground truth; repo docs used only for intent | 260 baseline capabilities |

The four references together yielded 786 feature records. Prevalence statements use N = 4, except the section 2 tables for the two browser categories, which use N = 3 because Apify has no browser shell to compare.

### A.2 Process

The work ran in seven steps.

1. **Inventory.** Each source was mapped first: installed files, bundles, plugins, manifests and locale strings for the two apps; the repository layout for Firecrawl; the documentation tree for Apify. The inventories were later corrected by the deep dives where the code disagreed with them.
2. **Deep dives.** Each source was split into two areas (Codex: in-app browser, automation and safety; Aside: browser product, agent-control architecture; Firecrawl: API surface, engine and infrastructure; Apify: platform) and turned into structured feature records, each with a mechanism, an evidence location (file and line, byte offset, or URL), a provenance tag and a confidence level.
3. **Code-verified baseline.** Two baselines described what Verevon actually has, one for Quarry v2 and integration-corev2 and one for the browser application path across the Model and Frontend planes. Each capability was tagged shipped, partial, stub or absent from the code, not the docs.
4. **Cross-source comparison.** The 786 records were grouped into 322 clusters across ten categories (acquisition API surface; output formats and content processing; rendering and browser engines; anti-bot, proxies and reliability; orchestration and scale; platform and ecosystem; observability and operations; in-app browser shell; AI-browser control; integration-plane overlap). Each cluster records which references have the capability, the best implementation, and Verevon's baseline status.
5. **Consolidation.** Per-category candidate recommendations were merged across categories (for example, production browser grants, crawl scoping and budget enforcement were each raised in three categories) into 77 items. Two candidates were dropped: design extraction (niche) and a Slack entry point (pre-empts the future Channel Plane).
6. **Tier calibration.** The category consolidators had proposed 121 candidates, 44 of them in Priority; the tiering pass cut Priority to 24 by merging duplicates and demoting items that do not block the vision, assigned waves, and listed the ADR topics.
7. **Two independent verification passes.** A Verevon-side pass read the code behind every one of the 77 items and recorded the exact gap, evidence paths, reusable parts, constraints, an adjusted approach and a revised effort. A reference-side pass re-read the evidence behind 24 items to refute or correct the stated reference behaviour and prevalence counts. The verified data supersedes the originals throughout this document.

Model tiers by job: inventories ran on Claude Haiku 5.5 at maximum effort; deep dives on Claude Sonnet 5.5 at extra-high effort; baselines and comparison on Claude Opus 5.5 at high effort; web research and writing on Claude Fable 5.1 at medium effort.

### A.3 How REA was used, and its limits

REA (reverse-engineering analysis) was run only on the two installed apps. Its contribution was smaller than planned, and the provenance tags reflect that honestly.

- **JavaScript analysis was partial.** On Codex, `analyze_javascript_application` parsed the plugin scripts and the computer-use modules without parse failures, but the semantic analysis of the two large bundles (`browser-service.mjs` and `browser-client.mjs`) exhausted the worker heap at the 2048 MB limit, so the dependency graph is partial and Electron surfaces reported zero [REA]. On Aside, the same tool hit the heap limit on the extension's `background.js` and `content.js` [REA].
- **Feature tracing returned no matches on strings that exist.** `trace_application_feature` seeds for literals and APIs present in the sources returned no-match on both apps because the partial graph does not index string literals in these minified bundles; absence of a match was never treated as evidence of absence [REA].
- **Native sessions failed on the MSIX install path.** Opening `codex-computer-use.exe` directly under `WindowsApps` was refused by the binary loader (Win32 access denied); a scratch copy opened, but `search_strings` timed out twice at 330 s [REA]. On Aside, `search_strings` on `aside-native.node` also timed out at 330 s [REA]. `inspect_pe_resources` worked on both apps and contributed manifest and resource facts, for example the `asInvoker` execution level of the Codex sandbox setup executable [REA].
- **Consequence.** Most app evidence is tagged SRC: beautified copies of the shipped bundles (names preserved, line numbers refer to the copies), byte-offset reads of the Aside daemon payload, parsed locale paks, extension sources and ASCII string tables of the native executables. No code was executed and no disassembly was done; native facts are string-table facts only [SRC]. Where a record rests on strings alone, its confidence is medium and the record says so.

### A.4 Limits that affect confidence

- **Server-side behaviour is invisible.** Codex's reviewer prompts, site-status reputation service, cloud browser, cloud broker and Chrome extension live outside the package; only their client contracts were seen, so their behaviour is DOC or INF [SRC]. Aside's remote-control trust rests on a cloud relay that is not visible locally [INF]. Firecrawl's hosted parts (fire-engine, FirePDF, Hangar, smart-scrape, the privacy service and the index database) are closed; what they do is known only through the open-source client code, so any "Firecrawl can do X" claim that is not in the repo is hosted-only [SRC].
- **Apify is public-docs-only.** No Apify code was read, so every Apify claim is DOC, and the docs themselves carry inconsistencies (for example two different free-plan concurrency figures) [DOC].
- **Presence of code is not proof of enablement.** Several Codex features are flag-gated (accessibility-tree mode in the in-app browser, raw CDP access, history, WebMCP), and client defaults stay off until server values load, so parity targets may not be fully shipped in the audited build [SRC]. Feature flags and rollout state in all four references are invisible.
- **Nothing was executed or launched.** No reference app, installer, test, build or dev server was run, and nothing in Verevon was run. Every "fails in production" statement about Verevon is an inference from reading the code and is tagged INF where it is not a direct code fact. Items that still need runtime confirmation are listed in the open questions.
- **Reference code evolves.** Firecrawl is pinned to one commit and the apps to one build; the in-repo Firecrawl OpenAPI is already stale against its own code, and SDKs drift from the API [SRC].
- **Licence caution.** The Firecrawl API is AGPL-3.0 and its Python SDK is MIT, so this document recommends re-implementing observed designs, never copying code; this is an engineering caution, not legal advice [SRC].
- **No user data was read.** Per-user data directories, cookies, local storage, tokens and real secrets were excluded for all apps; a few shipped files that carry key material were noted and not recorded.
- **Repo docs are stale.** Verevon docs claim robots handling, format parity, DOCX support and budget enforcement are done and mark the browser workspace plan complete; the code contradicts each claim. Code was treated as ground truth throughout.

### A.5 Counts

| Measure | Count |
|---|---|
| Reference sources | 4 |
| Reference feature records | 786 (Codex 261, Aside 187, Firecrawl 226, Apify 112) |
| Verevon baseline capabilities | 260 (142 shipped, 52 partial, 22 stub, 44 absent) |
| Comparison categories | 10 |
| Comparison clusters | 322 |
| Recommendations | 77 (2 candidates dropped) |
| Tiers before verification | 24 Priority, 41 Recommended, 12 Nice to have |
| Tiers after verification | 24 Priority, 35 Recommended, 18 Nice to have |
| Verevon-side verification verdicts | 8 keep, 61 reword, 8 demote (6 changed tier) |
| Verevon status after verification | 61 Partial, 3 Stub, 11 Absent, 2 Planned |
| Effort estimates revised | 33 |
| Reference-side checks | 24 (2 confirmed, 22 corrected in part) |
| Prevalence counts corrected | 14 |
| Items still unverified or low confidence | 0 |
| ADR topics | 10 |
| Delivery waves | 5 |
