# Norway data sources — roadmap notes

> **Superseded implementation guidance:** use `norway-data-sources-roadmap-audit-2026-07-20.md` and `norway-data-sources-roadmap.proposed.md` as the governing roadmap. This note is retained as historical context and must not be read as runtime truth.

> **Runtime addendum — 2026-07-21:** The rebuilt Docker artifact is running on
> `127.0.0.1:3190`. The current verified route matrix, missing provider
> credentials, and remaining restricted work are recorded in
> `norway-data-sources-completion-2026-07-21.md`. Do not infer provider access
> from this historical brainstorming note.

Brainstorm notes from a 2026-07-20 strategy discussion about extending Verevon's
grounding to Norway's public open-data commons. Not a committed spec — a
working map of where each new source should live, so the next person who
picks this up doesn't have to re-derive it.

**Standing rule (confirmed, matches this service's own design rule above):**
extend Verevon's existing cores — no new Plane, no new consumer products
(Hjemla-clone, mittanbud-clone, a public "ChatGPT for Norway") right now. This
service is a candidate landing spot for new sources; not every source
belongs here — see "Doesn't fit here" below.

## Existing capabilities (historical context only)

Weather (Yr/met.no), Traffic (Statens Vegvesen), and News (RSS) remain
`information-core` capabilities. Carrier adapters and shipment tracking belong
to `shipping-core` in the Ingestion Plane; Bring tracking is credential-backed
and is not a public/anonymous `information-core` capability.
Brreg org verification lives in `org-core`, not here (it's domain-specific to
org onboarding, not ambient reference data) — though `execution-core`'s agent
tool for ad-hoc company lookups currently calls the public Brreg API directly
instead of routing through `org-core`. Worth aligning to one Brreg client
eventually; not urgent.

## New candidates that fit this service

### Kartverket (open address lookup first)

The open Address REST API is implemented in `information-core` as a bounded,
provenance-bearing lookup. It must not be conflated with application-controlled
Matrikkel or Grunnbok access.

**Enhancement opportunity, not a new module:** `shipping-core` (Ingestion
Plane) already handles postal codes/addresses as part of Bring/UPS/DHL/FedEx
rate and booking flows. Once a Kartverket module exists here, it's worth
richer address/property resolution feeding into that existing shipping flow
rather than building a second, separate postal-code lookup.

### NVE (flood / landslide / hydrological risk)

Public, no OAuth, read-only, ambient/location-based — fits the design rule.
Recommend its own module (`hazards` or similar) rather than folding into
`weather`, since it's a different upstream API from met.no even though the
use case (environmental risk at a location) rhymes with weather. Pairs with
Kartverket for "is this property at risk" style questions once both exist.

### Entur (real-time public transport)

Public, no OAuth — fits the design rule. Entur's API already covers all of
Norway (it aggregates Ruter, Vy, and other operators nationally), so this
isn't an API limitation — but ship and validate scoped to Oslo-area journeys
first, then widen. Pairs naturally with the existing traffic module (how
people move, not just how vehicles move).

### SSB (Statistics Norway)

Public, no OAuth, read-only statistics API (population, prices, economic
data). Broadly useful across many customer verticals rather than tied to one
use case — flagged as a priority addition since it grounds general
knowledge-base and inbox answers the same way news/weather already do.

## Doesn't fit here — needs its own home

### Altinn / Maskinporten

Two-part, not a single drop-in. The token/consent layer is
identity-shaped (per-org OAuth-style delegation) — belongs in `auth-core`,
next to Vipps. The data unlocked once authorized would be information-core-
shaped once a token exists. Real customer signal already exists (Prokom,
19+ Norwegian municipalities) but this needs the auth-core piece built first,
so it's a heavier lift than anything in this doc.

### Doffin (public tender/anbud notices) — recommend a dedicated `tender-core`

Doffin itself might technically satisfy this service's design rule (public
notice search is unauthenticated), but the anticipated growth path —
combining with more procurement-adjacent providers, going deeper on
tender-document analysis — argues for a dedicated core rather than growing
inside this one, the same reasoning that keeps ERP integrations out of here
(see design rule above). Same plane (Application Plane), just its own
bounded service so this one stays lean.

Flagged explicitly: Doffin edges toward Cobrief's competitive space
(tender/anbud tooling). Fine as a lookup/grounding tool; would need a
separate conversation before it became anything more product-shaped.

### Lovdata (law text)

Different in kind from everything else in this doc — a full-text legal
corpus, not a live JSON API to proxy. If/when access terms are confirmed to
allow it (historically more restricted/commercial than the sources above —
verify current terms before committing to anything), this belongs in
**Data Plane v2** as ingested/embedded/cited content (crawled and indexed
like any other knowledge source), not as a microservice core or an
`execution-core` tool call.

### Folkeregisteret / Skatteetaten (personal data)

Deliberately left out. Rather than building any integration, Verevon should
refer users to the official source directly when this comes up — cleaner,
and consistent with the data-minimization stance already in the Trust
Center's data classification. Revisit only if a real legal-basis case
appears, and treat it with the same rigor as any other sensitive-personal-
data class, not as "just another lookup tool."

## More candidates (2026-07-20 follow-up)

Confidence noted per source — these are well-established from general
knowledge, not verified against live docs this session. Check current terms
before committing, the same discipline already applied to Lovdata above.

### High confidence, strong fit — `information-core`

- **Geonorge** — the national geodata portal, and notably the **open/free
  sibling to Kartverket's more restricted matrikkel/grunnbok API**. Address
  and property-location lookups are reachable here without the data-sharing
  agreement Kartverket's own Eiendomsregisteret API requires. Worth building
  the first Kartverket-shaped module against Geonorge, and treating a formal
  Kartverket agreement as a later upgrade for anything Geonorge can't answer
  (grunnbok/tinglyst rights specifically).
- **Storting API** (`data.stortinget.no`) — bills, votes, representatives,
  committee proceedings. Complements Lovdata: Lovdata is law *text*, Storting
  is the law-*making* process (what's currently being debated/voted on) —
  and unlike Lovdata this one is genuinely open, no commercial gate.
- **Frost API** (`frost.met.no`) — MET Norway's historical/observational
  weather-station data, distinct from the Yr forecast API already in use.
  Sibling module to `weather`, but backward-looking (actuals) instead of
  forward-looking (forecast).
- **NVDB — Nasjonal vegdatabank** (Statens Vegvesen) — road network geometry,
  speed limits, and road attributes. Distinct from the traffic-counting-
  station data already used in the `traffic` module; a sibling module, not a
  replacement.
- **eInnsyn** (`api.einnsyn.no`) — searches public-sector case documents and
  postjournals across Norwegian public bodies. Directly relevant to the
  Prokom/municipality angle and the same competitive space as Ayfie's
  "Innsynsagenten" example already in the research.

### Enhancement to what's already built, not a new module

- **Brreg's own sub-registers** — the existing `org-core` Brreg client only
  covers basic company lookup today. The same Brreg API family also exposes
  Regnskapsregisteret (filed annual accounts), Konkursregisteret (bankruptcy
  status), and Frivillighetsregisteret (non-profit registry). Worth checking
  whether `org-core`'s client already reaches these or only the base entity
  lookup — likely a small extension, not a new integration.

### Medium confidence / real but narrower audience

- **NAV "Arbeidsplassen" API** — job vacancy listings. Relevant only if an
  HR/recruitment-adjacent use case shows up.
- **Miljødirektoratet** — environmental/pollution open data. ESG-adjacent,
  niche today.
- **Riksantikvaren / Kulturminnesøk** — protected heritage-site register.
  Niche, property/tourism angle only.
- **Kystverket AIS** — maritime vessel-traffic data. Only relevant for a
  maritime-logistics customer specifically.

### Not a data source — a discovery mechanism

- **data.norge.no** (Digdir's national open-data catalog) — the systematic
  way to keep finding new Norwegian open datasets going forward, rather than
  this doc trying to enumerate everything by hand. Worth a periodic check
  rather than a one-time list.

## Suggested sequencing

1. Geonorge / Kartverket — start with Geonorge (open, no agreement needed)
   for address/property lookups; treat a formal Kartverket agreement as a
   later upgrade for grunnbok/tinglyst rights specifically.
2. NVE, Entur, SSB, Frost, NVDB, eInnsyn — same pattern, roughly
   interchangeable order, all `information-core`.
3. Brreg sub-registers — check what `org-core`'s existing client already
   covers before treating this as new work.
4. Doffin — once there's appetite for a second core, not urgent.
5. Maskinporten — pull-driven by the Prokom relationship; needs `auth-core`
   work first, so plan for it separately rather than alongside the above.
