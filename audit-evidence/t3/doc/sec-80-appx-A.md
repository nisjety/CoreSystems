## Appendix A. Method, sources and limits

### A.1 What was analysed

Twelve reference sources were analysed: six installed desktop apps, two open-source repositories and four public web products. Verevon itself was read at CoresSystem commit afb83928 (2026-10-10). The table gives each source's version, kind, analysis method and the number of structured feature records it produced.

| Source | Version or commit | Kind | How it was analysed | Feature records |
|---|---|---|---|---|
| Claude desktop app | 2.31226.1 | installed app | REA on app.asar plus the extracted bundle | 449 |
| Hermes Agent | commit b23600860f (2026-10-12) | open-source repo | source and docs | 278 |
| DeepSeek harness | commit d743267388 (2026-10-09) | open-source repo | source and docs | 335 |
| T3 Code | 0.0.35 | installed app | REA on app.asar plus recovered sources | 203 |
| Manus (Manus Studio) | 2.0.6 | installed app | REA on app.asar (targeted subdirectories) plus the extracted bundle | 218 |
| Codex desktop app (plugins, agent runtime, models) | 26.1007.2314.0 | installed app | REA on app.asar and .vite build plus native binary strings | 227 |
| Google Antigravity (agent hub) | 2.11.0 | installed app | REA plus direct reading of the embedded UI and protobuf descriptors | 247 |
| Cursor | 3.17.21 | installed app | REA on extension bundles plus direct reading | 287 |
| manus.im | public docs, October 2026 | public documentation | public web docs only (DOC) | 105 |
| perplexity.ai | public docs, October 2026 | public documentation | public web docs only (DOC) | 112 |
| chatgpt.com | public docs, October 2026 | public documentation | public web docs only (DOC) | 155 |
| claude.ai | public docs, October 2026 | public documentation | public web docs only (DOC) | 127 |

### A.2 The process

The audit ran in seven steps.

1. **Inventory.** Each source was inventoried once: install layout, manifests, bundles, binaries, documentation and, for the web products, the public help and blog pages.
2. **Deep dives.** Each installed app and repository was split into two facets (for example product UX and runtime architecture for Claude). Each facet produced a narrative and a JSON file of feature records; every record carries its category, evidence (file, line or page), provenance tag and quality rating. The deep dives wrote their own limitations sections, which A.3 and A.4 draw on.
3. **Code-verified Verevon baseline.** Three baselines (Model Plane, chat and agents UI, planned work and decisions) were built by reading the CoresSystem code and decision ledgers. Each of the 500 records states shipped, partial, absent, stub or planned, with a code path. Because many repository documents are stale, code was treated as ground truth and documents as intent.
4. **Cross-source comparison.** The 2,743 reference records were grouped into 595 capability clusters across 13 categories. Each cluster records prevalence, the best implementation and the Verevon status.
5. **Consolidation.** Each category's clusters became recommendations (159, R-t3-*) with a gap statement, the reference best practice, an approach and an effort estimate.
6. **Tier calibration.** One pass merged duplicates across categories (the same defect often surfaced in several), assigned tier, rank and wave, and produced the 98 final items (F-t3-*); two items were dropped.
7. **Two independent verification passes.** A Verevon-side pass read the code for every item and rewrote status, exact gap, evidence paths, reusable parts, constraints, approach and effort. A reference-side pass re-opened the cited evidence for every Priority item and tried to refute it, correcting over-reach and prevalence counts. Verified data supersedes the earlier drafts throughout.

Model tiers by job: inventories ran on Claude Haiku 5.5 at maximum effort; deep dives on Claude Sonnet 5.5 at extra-high effort; the baseline and the cross-source comparison on Claude Opus 5.5 at high effort; web research and document writing on Claude Fable 5.1 at medium effort.

### A.3 How REA was used, and its limits

REA (the reverse-engineering analysis toolset) was run only on installed apps and only against read-only copies: extracted app.asar archives, Vite or Electron build directories and PE executables. It was not used on Hermes or DeepSeek, which are source repositories [SRC]. Its JavaScript analysis (`analyze_javascript_application`) produced module graphs, IPC inventories and findings, and `inspect_pe_resources` confirmed integrity resources, version data and the absence of symbols on native helpers [REA].

Its limits were consistent across the apps. On large minified bundles the semantic pass hit the 2 GB heap: Antigravity's main.js exhausted the heap and left the graph partial [REA]; Claude's whole-app run exhausted the semantic budget, so the deep dive analysed a targeted subset and left out the 6.9 MB core chunk [REA]; Codex's code-review plugin ran out of heap on two files [REA]; Manus coverage was reported partial with 355 limitations [REA]. String-seed tracing (`trace_application_feature`) does not index string literals in these minified graphs, so seeds that `grep` found directly returned no match for Claude, T3 Code and Antigravity; the tools state that this is not evidence of absence [REA]. As a result, most app evidence in this audit is tagged SRC: the deep dives read the extracted or beautified bundles directly and used REA for structure. Native binaries were examined through PE resources and string extraction only; the Codex Rust runtime, the Manus Go and Rust helpers and the Cursor sandbox helper were never decompiled or run, so claims about their behaviour rest on names, messages and prompt text [SRC, INF]. The Claude copy is the Windows MSIX build, so the macOS and Linux variants (Swift helper, bubblewrap) were not available [SRC]. Cursor had no pre-extracted copy; the analysed files were a scratch copy of its `out/` directory [SRC].

### A.4 Limits that affect confidence

- **Server-side behaviour is invisible.** Every installed app is a client. Manus's cloud side (agent, sandbox VM, policy engines) is absent from the bundle; Cursor's models, routing and review services were not observable; Codex's sandbox, approval and model logic sits in the Rust runtime. Statements about what these servers do are DOC or INF [SRC].
- **Web products are public-docs-only.** claude.ai, chatgpt.com, perplexity.ai and manus.im were read from help pages, blogs and press. Their claims are DOC and were not exercised; one page could not be re-fetched during verification (HTTP 403).
- **Nothing was executed or launched.** No app, installer, project code, test, build or dev server was run; no user data, cookie or credential store was read. Runtime-only behaviour (feature flags, server-pushed configuration, consent flows) is invisible; several Cursor and Claude mechanisms are flag-gated with server-controlled values and are reported as such.
- **Minified code reads are lossy.** UI behaviour in Manus and T3 Code was partly reconstructed from i18n keys; Claude's instruction-scope list comes from label strings, not assembly code.
- **Verevon documents are partly stale.** One baseline (connector reads) was corrected by a re-read on 2026-10-14. Verified item data wins over the planning documents quoted in cards.
- **Reference code evolves.** Repositories and apps were read at the commits and versions above; later releases may close or open gaps.
- **Prevalence counts are approximate.** Verification found 14 wrong counts among the Priority items; corrected counts are used where available and omitted otherwise.

### A.5 Counts

- Reference feature records: 2,743 across 12 sources.
- Verevon baseline records: 500 across 13 categories (290 shipped, 122 partial, 57 absent, 9 stub, 22 planned).
- Capability clusters compared: 595 in 13 categories.
- Consolidated recommendations: 159 (R-t3-*), merged into 98 final items (F-t3-*), 2 dropped.
- Tiers: 23 Priority, 45 Recommended, 30 Nice-to-have before verification; 22, 46 and 30 after.
- Verevon-side verification: 98 items read; verdicts keep 13, reword 81, demote 4; effort revised on 40; one tier change; 0 items still unverified, 0 low-confidence.
- Reference-side verification: 23 Priority items re-checked; 2 confirmed, 21 partially confirmed with corrections; 14 prevalence counts corrected.
- Verified Verevon status of the 98 items: 78 Partial, 16 Absent, 2 Stub, 2 Planned.
