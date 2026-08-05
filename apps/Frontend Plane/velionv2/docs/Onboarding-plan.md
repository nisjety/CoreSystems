# Goal: Make Onboarding Prove Verevon Understands The Company Before Paywall

Verevon onboarding should feel like an AI system building the first operational picture of the company, not a pricing wizard. Each step must show what Verevon learned, keep sensitive data out of the first-run experience, and give the user an obvious way to undo every onboarding choice.

## Product Principles

- Verevon is the AI-first operator across support, sales, knowledge, routing, workflows, and deployment. The UI should show what Verevon can do and where human review is still required.
- Onboarding inspectors are read-only. Source edits, graph edits, and audited corrections belong in Knowledge and Settings after onboarding.
- Data Plane v2 graph-index is the source of truth for real graph evidence. Optimistic UI nodes can show pending/importing states but must not be used as proof.
- Model Plane recommendations must be non-blocking. Local proof renders instantly, graph/model evidence refines it when available, and failures must never stop plan selection.
- Every onboarding action must be reversible: remove websites, switch back to Verevon theme, disconnect connectors, choose a different plan, or continue after pending cleanup.

## Website Step

- The crawl step does not auto-advance after SSE completion.
- The user can continue after the first crawl signal or a terminal crawl state, add another website, remove a website, or skip.
- `state.website` remains the primary website for compatibility. `state.additionalWebsites` stores extra crawled sites.
- Crawl evidence is bounded and safe: status, page counts, element counts, content types, warnings, and capped snippets.
- The right panel shows what Verevon can use from the crawl and hints that audited edits happen later in Knowledge.

## Organization And Brand Theme

- The org inspector shows public identity evidence: logo, domain, BRREG facts, employee count, palette, and theme choices.
- Users can choose Verevon theme or the detected brand theme. Brand theme uses `branding.themeColor` first, then the first valid palette color.
- The selected accent is applied immediately, persisted in onboarding state, and best-effort saved to workspace appearance settings.
- Failed appearance saves are non-blocking: the local preview stays active and the user can retry or switch back.
- Organization names are normalized for display so all-caps legal names read cleanly while legal suffixes like AS remain uppercase.

## Integration And Data Plane v2

- After OAuth succeeds, onboarding requests safe provider metadata discovery and seeds Data Plane v2 with only high-level evidence.
- Discovery is provider-specific and bounded:
  - Slack: workspace/team name, visible public channel count, up to three public channel names if allowed, availability flags.
  - GitHub: owner/org, repo count, up to three repo names, README/issues/wiki availability.
  - Notion: workspace name, page/database counts, up to three allowed top-level names.
  - Microsoft 365/SharePoint: tenant name, site counts, broad availability/storage signals via Finspo-core when needed.
  - Google Drive: workspace/drive name, counts, and up to three allowed top-level folder names.
- Sensitive onboarding discovery must not return message contents, private channel names, document names, email subjects, file contents, code, issue bodies, or private file names.
- Connector removal should disconnect the provider where possible, remove pending onboarding seeds, and mark delayed backend cleanup as pending instead of trapping the user.
- Onboarding graph/document seeds are cleaned up through a best-effort `source-cleanup` path. If Data Plane has already returned a document id, the BFF deletes that org-scoped document; otherwise the UI treats cleanup as pending and keeps the user moving.

## Right Panels

- Right panels act as operational inspectors for the current step.
- Inspectors show a read-only label, compact metrics, status, safe evidence, and only reversible onboarding actions.
- The integration graph supports zoom, pan, node click, selected-node inspection, and graph controls, but graph structure and node details are not editable in onboarding.
- The website inspector shows crawl evidence; the org inspector shows identity and theme; the integration inspector shows safe metadata and graph state; the paywall shows proof of concept.

## Paywall Proof Of Concept

- The paywall should show proof before asking for money:
  - Company identity: logo, normalized name, domains, employee count, selected theme.
  - What Verevon learned: crawled pages/elements, content types, safe source metadata, real graph counts.
  - Likely customer intents: inferred from user brief, crawl snippets, connectors, and safe graph metadata.
  - What Verevon can do next: create chatbot, draft replies, build macros, connect Shopify/Zendesk/SharePoint, deploy widget, and suggest workflows.
  - Expected first impact: directional ranges only, with human review categories.
  - Why the plan fits: tied to websites, source count, team size, workflow/risk needs, and plan capacity.
- The user can still choose a lower plan. If plan capacity requires fewer active connectors, onboarding should explain which sources will be disconnected and let the user continue.

## Completion Metadata

Onboarding completion should include safe metadata only:

- selected theme mode, primary color, and save status
- website URLs plus bounded crawl counts/status/content types
- connector IDs/labels plus metadata status, sensitivity marker, and onboarding seed document id when Data Plane returned one
- recommendation plan/source/timestamp

This metadata can power notifications and first dashboard setup without exposing private content.

## Remaining Backend Contracts

- Data Plane v2 should provide idempotent seed creation/deactivation for onboarding source evidence.
- Integration-core and Finspo-core should expose provider-specific safe metadata endpoints so frontend BFF routes do not need provider-specific guesses.
- Graph-index should clearly distinguish real, pending, failed, and removed onboarding source nodes.
- Model Plane should accept the extended onboarding context and return an optional `proofOfConcept` object, while preserving existing recommendation fields.

## Verification Coverage

- Unit coverage should protect multi-website add/remove, crawl evidence caps, theme resolution, org-name display, safe metadata normalization, and recommendation context safety.
- API coverage should verify org/session scope, non-blocking discovery errors, idempotent removal, real-vs-pending graph states, and sensitive-field stripping.
- Component coverage should verify no website auto-advance, reversible theme/connector changes, read-only graph inspector behavior, and paywall updates after going back.
- E2E coverage should exercise website crawl inspection, adding/removing websites, theme switching, mocked provider discovery/removal, and paywall completion when Model Plane or graph-index is unavailable.
