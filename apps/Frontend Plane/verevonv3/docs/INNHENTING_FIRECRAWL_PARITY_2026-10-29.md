# Innhenting: Firecrawl-paritet, det Quarry allerede tilbyr, og det som mangler

Dato: 2026-10-29. Gjelder siden `/ingestions` i Verevon v3 og gatewayen den kaller.

Målet er at Quarry v2 skal tilby alt Firecrawl tilbyr, pluss mer. Denne filen
sier, for hver funksjon, om den er koblet til ekte Quarry i dag, hva som må på
plass for å slå den på, og hvor i koden det er. Alt som vises i siden kaller en
ekte rute. Det som ikke finnes i Quarry, står her og vises ikke som knapp.

## Hvordan siden snakker med Quarry

Nettleseren kaller gatewayen (`/api/...`). Gatewayen kaller Quarry-edge med
brukerens token, og org-scope kommer fra sesjonen, aldri fra en klientheader.
Handlinger som endrer noe går gjennom `executeAction` mot action-kontraktene i
`src/shared/actions/action-registry.ts`. Hver kontrakt har en dispatcher i
`apps/gateway/src/domains/actions/`.

## Status per funksjon

Status: **Koblet** = ekte rute, fungerer i dag. **Flagg** = ekte rute, men
Quarry-bryteren er av på live, og siden viser Quarrys egen melding om hva som
mangler. **Mangler** = Quarry har det ikke ennå; se avsnittet "Mangler i Quarry".

| Firecrawl-funksjon | Quarry | Status | Hvor |
|---|---|---|---|
| Scrape: Markdown, HTML, rå HTML, lenker | `POST /v1/scrape` | Koblet | `knowledge/quarry.rs` (body går videre), `playground.ts` |
| Scrape: skjermbilde | `formats: screenshot` | Flagg `QUARRY_EDGE__SCRAPE_CAPTURE` | `scrape_actions.rs` |
| Scrape: PDF | `formats: pdf` | Flagg `QUARRY_EDGE__SCRAPE_CAPTURE` | samme |
| Scrape: main content / inkluder / ekskluder tagger | `only_main_content`, `include_tags`, `exclude_tags` | Koblet | `scrape.rs` |
| Scrape: vent og tidsavbrudd | `render.waitForTimeoutMs`, `timeout_ms` | Koblet | |
| Scrape: nettleserhandlinger (klikk, skriv, rull, velg, trykk, vent) | `actions`, `actions_on_error` | Flagg `QUARRY_EDGE__SCRAPE_ACTIONS` | `scrape_actions.rs`, `ScrapeAction` i `quarry-core/src/scrape_actions.rs` |
| Scrape: utdragsmodus (Quarry har mer enn Firecrawl) | `extractor`: `readability`, `clean_full`, `raw` | Koblet | `scrape.rs` |
| Scrape: fjern base64-bilder (Quarry) | `remove_base64_images` | Koblet | |
| Scrape: maks alder / cache | `max_age` i millisekunder; svaret viser `cache_state`, `cached_at` og `cache_age_ms` | Flagg `QUARRY_EDGE__SCRAPE_MAX_AGE` (live: av). Koblet i Verevon: «Bruk lagret resultat opptil», og kilden vises under resultatet | `scrape_cache.rs` (Quarry), `playground.ts`, `scrape-view.ts` |
| Scrape: JSON-format med skjema per side | `formats: {type:'json', schema}` | Flagg `QUARRY_EDGE__AI_FORMATS` (live: av). Koblet i Verevon; Quarry-siden på `quarryv2-next` (a8331b38c, 15ca48ed8, d4fa6ee08) | `quarry-core/src/scrape.rs`, `playground.ts`, `IngestionsPlaygroundResult.tsx` |
| Scrape: sammendrag, spørsmål, høydepunkter | `formats: {type:'summary'}`, `{type:'question',question}`, `{type:'highlights',query}` | Flagg `QUARRY_EDGE__AI_FORMATS` (live: av). Koblet i Verevon | samme |
| Scrape: branding (farger, logo) | feltet `branding` finnes i svaret | Koblet (vises i JSON og metadata) | `pipeline.rs` |
| Scrape: lagre i Kunnskap | `ingest: true` | Koblet ("Send til Kunnskap") | `knowledge/quarry.rs` |
| Map: finn URL-er på et nettsted | `POST /v1/map` | Koblet | `knowledge/quarry.rs` (`crawl/discover`) |
| Search: nettsøk | `POST /v1/search` | Koblet | `search.rs` |
| Search: tidsrom, emne, domener, eksakt treff | `time_range`, `topic`, `include_domains`, `exclude_domains`, `exact_match` | Koblet | `search.rs` |
| Search: marked og språk (Quarry) | `country`, `language` | Koblet (lagt til i denne runden) | `search.rs` |
| Search: svar generert fra treffene | `include_answer`, `answer_status`, `answer_reason` | Koblet i Verevon: viser svaret, eller hvorfor det mangler. Bryter `QUARRY_EDGE__ANSWER_SYNTHESIS` (live: ikke slått på) | `search.rs`, `errors.ts` (`answerStatusMessage`) |
| Search: skrape alle treff | `POST /v1/batch` med treffene | Koblet | |
| Crawl: følg lenker, dybde, stier, underdomener | `max_pages`, `max_depth`, `include_paths`, `exclude_paths`, `allow_subdomains` | Koblet | `crawl_request.rs` |
| Crawl: sitemap | `sitemap`: `skip`, `include`, `only` | Flagg `QUARRY_EDGE__CRAWL_SITEMAP` (live: av). Koblet i Verevon | `knowledge/quarry.rs` (`normalize_crawl_body_for`), `playground.ts` |
| Crawl: skrape-valg per side | `scrape_options` | Flagg `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` (live: av). Koblet i Verevon: innholdsvalgene gjelder hver side | `knowledge/quarry.rs`, `playground.ts` |
| Crawl: pause, gjenoppta, avbryt | `POST /v1/crawl/{id}/pause` m.fl. | Flagg `QUARRY_EDGE__JOB_CONTROL` | `job_lifecycle_routes.rs` |
| Batch: liste med URL-er | `POST /v1/batch` | Koblet, også pause/avbryt | |
| Extract: strukturert data med JSON-skjema | `POST /v1/extract`, `schema` | Flagg `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` (live: 501 med hint) | `extract_routes.rs` |
| Extract: fri prompt | `prompt` | Koblet i Verevon. Quarry-siden bruker den (5f1ac3ca2, ingen flagg) | `extract_routes.rs`, `runs.rs` (`extract_prompt`) |
| Agent: autonom nettleseragent | `/v1/agent/runs` via Verevon | Koblet via `ingestions.start_agent_run` | `dispatchers.rs`, `browser.rs` |
| Agent: direkte Quarry-API uten Verevon | krever nettleser-grant fra gatewayen | Mangler (se under) | `agent_routes/lifecycle.rs` |
| Monitor: sjekk side for endring | `POST /v1/change/check` | Koblet | `change_routes.rs` |
| Monitor: gjentakende overvåker (time, dag, uke) | `POST /v1/schedules` med `target_kind=change_monitor` | Koblet (opprett/stopp) | `monitoring.rs` |
| Monitor: tekstdiff mellom versjoner | `GET /v1/change/diff` | Flagg `QUARRY_EDGE__CHANGE_TEXT_DIFF` (live: av). Koblet i Verevon: «Vis endringen» | `monitoring.rs`, `ChangeDiffPanel.tsx` |
| Monitor: varsel i appen | `change_detected` via control | Koblet (listes i Overvåking) | `notify/` |
| Monitor: varsel på e-post / webhook | `/v1/webhook-subscriptions` | API på lanen bak `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` (av). Ingen UI ennå: sikkerhetsgjennomgang pågår | |
| Tidsplaner: cron, pause, utløs | `/v1/schedules` | Koblet; lagring er av på live (se under) | `schedules.rs` |
| Jobber: liste, status, feil per URL, robots-blokkert | `/v1/jobs/{id}`, `/v1/jobs/{id}/errors` | Koblet (ny i denne runden) | `ingestions/jobs.rs` |
| Jobber: opprinnelse (manuell eller tidsplan) | `origin`, `schedule_id` | Koblet (ny i denne runden) | `ingestions/runs.rs` |
| Kjøring: sider som ble hentet (Apify-datasett) | `GET /v1/jobs/{id}/results`, `/v1/crawl/{id}/results`, `/v1/batch/{id}/results` | Flagg `QUARRY_EDGE__JOB_RESULTS` (live: av). Koblet i Verevon: «Sider som ble hentet», med markdown per side | `ingestions/jobs.rs`, `IngestionsRunsView.tsx` |
| Forbruk: kreditter for perioden | `GET /v1/team/credit-usage` | Koblet (ny i denne runden) | `ingestions/jobs.rs` |
| Parse: dokument (PDF, DOCX) til Markdown | intern opptak, ikke offentlig parse-rute | Mangler (se under) | `pipeline.rs:1043` |
| Browser-økter og profiler | `/api/v1/browser/*` | Koblet i Nettleser (ikke i denne siden) | `browser.rs` |

## Flagg som må slås på for å bruke de grå feltene

Dette er bryterne på Quarry-edge. Siden er koblet til dem, så når en bryter
slås på, virker funksjonen uten endring i koden.

| Bryter | Hva den låser opp | Status på live |
|---|---|---|
| `QUARRY_EDGE__SCRAPE_ACTIONS=on` | nettleserhandlinger i scrape | av |
| `QUARRY_EDGE__SCRAPE_CAPTURE=on` | skjermbilde og PDF | av |
| `QUARRY_EDGE__JOB_CONTROL=on` | pause, gjenoppta og avbryt av crawl og batch | av |
| `QUARRY_SCHEDULE_CAPABILITIES` (JSON) | tidsplantyper, utløs og backfill | tom: lagring av tidsplaner er av |
| `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` | AI-formater på scrape (json, summary, question, highlights) og strukturert uttrekk | av: AI-formater gir 400 `feature_disabled`, `/v1/extract` gir 501 med hint |
| `QUARRY_CONTROL__JOB_CREDITS=on` | kreditter per jobb (`credits`), default av | av: feltet mangler i svaret, og siden sier det |
| `QUARRY_EDGE__SCRAPE_MAX_AGE=on` | `max_age` på scrape, med Redis for hurtiglageret | av: 400 `feature_disabled` på `max_age` |
| `QUARRY_EDGE__CRAWL_SITEMAP=on` | `sitemap` på crawl | av: 400 `unsupported_option` på `sitemap` |
| `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS=on` | `scrape_options` på crawl | av: 400 `unsupported_option` på `scrape_options` |
| `QUARRY_EDGE__CHANGE_TEXT_DIFF=on` | `GET /v1/change/diff` og markdown på basislinjer | av: 501 |
| `QUARRY_EDGE__JOB_RESULTS=on` | sider per kjøring | av: 501 |
| `QUARRY_EDGE__ANSWER_SYNTHESIS=on` | svar i søk (`answer_status`) | av: `answer_status` er `unavailable` med `synthesis_disabled` |
| `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS=on` | abonnement på endringswebhook | av: 501 (ingen UI ennå) |

Ingen av disse er slått på i denne runden. Å slå dem på er en konfigurasjonsendring
på den delte live-stacken, og det må godkjennes av eieren.

## Mangler i Quarry

Hver linje peker til stedet i Quarry-koden. Nummereringen er den samme som før, så den kan sammenlignes med tidligere versjoner av dette dokumentet. Quarry-punktene fra den andre leveransen står i parentes der de tilsvarer en linje her.

0. **Live-gatewayen er ikke oppdatert.** Den kjører build fra `14299e659`. Endringene fra `b2bf63e2a` og fra denne runden er testet med cargo, wiremock og vitest, ikke på live. Å bytte live krever eierens godkjenning.
1. **AI-formater på scrape (`json`, `summary`, `question`, `highlights`).** Levert på `quarryv2-next` og på lanen `innh/lane-a` (336b1da1b). Begge blokkerne er lukket: `truncated_input` settes over 60 000 tegn, og en personvernblokk (`credential_or_secret`, `zdr_ephemeral`) gir `skipped` med `PRIVACY_POLICY` uten modellkall. Fortsatt av på live: `QUARRY_EDGE__AI_FORMATS` er ikke slått på, og modellplanen er ikke koblet.
2. **Fri `prompt` på `/v1/extract`: levert** (5f1ac3ca2). Prompten brukes nå, også uten skjema.
3. **Tekstdiff for overvåking (Quarry-punkt 4).** Levert på `innh/lane-b` (c9cca3667, 9455099e3, 96c5dc9c3): markdown-artefakt på basislinjen og `GET /v1/change/diff`, bak `QUARRY_EDGE__CHANGE_TEXT_DIFF` (av). SQL-en er ikke prøvd mot Postgres.
4. **Abonnement på endringswebhook (Quarry-punkt 5).** API-et er bygget på `innh/lane-b` bak `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` (av). Ingen UI i Verevon ennå: sikkerhetsgjennomgangen i Quarry er ikke ferdig, og Quarry sender endelig form.
5. **Sitemap og skrape-valg for crawl (Quarry-punkt 6).** Levert på `innh/lane-c` bak to flagg (av). Rulles ut i rekkefølgen i RUNBOOKS: orkestratoren først, så edge-flagget.
6. **Maks alder på scrape (Quarry-punkt 7).** Levert på `innh/lane-a` (31d9ad15f, 5119e6ef8): `max_age` i millisekunder og `cache_state` i svaret, bak `QUARRY_EDGE__SCRAPE_MAX_AGE` (av). Uten Redis gir den `bypass` med advarselen `cache_unavailable`.
7. **Svar i søk (Quarry-punkt 8a).** Levert på `innh/lane-d` (ab65682aa, 0aa5cbbb7): `answer_status` og `answer_reason`. Live svarer `synthesis_disabled` helt til bryteren `QUARRY_EDGE__ANSWER_SYNTHESIS` er slått på. Det er en konfigurasjon, ikke kode.
8. **Kostnad per kjøring.** Levert på `quarryv2-next` (6bbd99763), bak `QUARRY_CONTROL__JOB_CREDITS` (av).
9. **Direkte agent-API for Quarry-kallere.** `/v1/agent/runs` krever `grant_id` fra gatewayen (`agent_routes/lifecycle.rs`), så en ekstern klient kan ikke starte en agent direkte. Designnotat for API-nøkkelgrants ligger på `innh/lane-d` (a7ceb6da8, punkt 10 i Quarry-listen). Ikke bygget.
10. **Parse av dokumenter til Markdown.** Pipeline-en leser PDF og DOCX (`pipeline.rs:1043`), men bare som opptak til kunnskapsbasen. Må: en offentlig rute som returnerer Markdown uten å lagre. Quarry-sesjonen bygger dette nå.
11. **Resultater per side for en jobb (Quarry-punkt 12).** Levert på `innh/lane-b` (cc2596c7a, 4f2a1bc3c): `GET /v1/jobs/{id}/results`, `/v1/crawl/{id}/results` og `/v1/batch/{id}/results`, bak `QUARRY_EDGE__JOB_RESULTS` (av), og `GET /v1/artifacts?run_id=&kind=`. Ikke prøvd mot et stack.
12. **Schedule-kinds på live.** `QUARRY_SCHEDULE_CAPABILITIES` er tom, så siden kan ikke lagre tidsplaner. Må: konfigurasjon, etter at utførelsen er verifisert.

## Endringer i denne runden

Runden 2026-10-31 kobler punktene 1, 2 og 9 fra Quarry-listen til gatewayen og siden:

- Gateway: `POST /api/v1/knowledge/scrape` videresender AI-format-objekter uendret
  (testet med wiremock). Scrape med AI-format får 135 s klientgrense, over edgens 125 s.
  `summary` som ren streng teller også som AI.
- Gateway: uttrekk videresender `prompt` (tomt utelates, over 10 000 tegn gir 422).
- Gateway: `credits` på jobbrader (`/api/ingestions/runs`) og jobbdetalj, camelCase, og
  fraværende når Quarry ikke sender det.
- Frontend: scrape med `json` (skjema og valgfri instruks), `summary`, `question` og
  `highlights`. Svaret viser strukturert JSON, sammendrag, svar og høydepunkter med
  tegnposisjoner. AI-rapporten viser status per format med produkttekst for ZDR_UNAVAILABLE,
  BUDGET_EXHAUSTED, MODEL_RATE_LIMITED, MODEL_UNAVAILABLE, SCHEMA_INVALID og EMPTY_SOURCE,
  og en tydelig merking av at svaret er data fra siden.
- Frontend: uttrekk har et eget prompt-felt. Resultatet viser skjemastatus, skjemabrudd og
  avkortet input per side.
- Frontend: refusjoner navngir bryteren som er av: `QUARRY_EDGE__AI_FORMATS` for AI-formater,
  `QUARRY_EDGE__SCRAPE_CAPTURE` for skjermbilde og PDF, og viser 403 `extract_ai_scope_required`
  og 501 `model_plane_unavailable` / `model_lane_not_granted` med egen tekst.
- Frontend: kjøringsdetaljen viser kreditter med «estimert» og «endelig», eller hvorfor de mangler.

Ikke verifisert: en ekte AI-kall. Quarry-siden er testet mot en modelldobbel, og live har
ikke AI-formater på. Skjermbilde av AI-svar på live er ikke tatt.

- Gateway: nye ruter `GET /api/ingestions/jobs/{id}`, `GET /api/ingestions/jobs/{id}/errors`,
  `POST /api/ingestions/jobs/{id}/control` og `GET /api/ingestions/usage`
  (`apps/gateway/src/domains/ingestions/jobs.rs`).
- Gateway: `extract` videresender `schema` (den sendte `prompt` ble droppet av
  Quarry og er fjernet fra kontrakten). Ukjent JSON i `schema` gir 422.
- Gateway: kjøringer får `origin` og `scheduleId`. En extract-kjøring er `failed`
  når kilden feilet, ellers `completed`.
- Gateway: `/api/v1/search/web` videresender `country` og `language`, og de er
  med i cache-nøkkelen.
- Action-kontrakter: `ingestions.job_control` (pause, gjenoppta, avbryt) og
  `ingestions.start_agent_run` (åpner nettleserøkt og starter agent). `ingestions.create_run`
  tar `schema` i stedet for `prompt`.
- Frontend: alle stubber er borte. Kostnad, opprinnelse og "per side" er erstattet
  med ekte data eller er fjernet. Tekstdiff og e-post/webhook er ikke i siden.
  Det som mangler, står i denne filen.

## Endringer i runden etter (andre leveranse fra Quarry)

Gateway (`apps/gateway`):
- `GET /api/v1/monitoring/diff` går til `/v1/change/diff` og gir `sourceUrl`, `from`, `to`, `diffId`, `available`, `reason`, `diff` og `markdown`.
- `GET /api/ingestions/jobs/{id}/results` gir `items` (`seq`, `url`, `status`, `markdownArtifactId` m.fl.), `nextSeq` og `scanCapped`.
- `GET /api/ingestions/artifacts/{id}` gir tekst for `text/*` og `application/json`, kuttet på 1 MiB. ZDR gir samme svar som skjermbilder.
- Crawl: `sitemap` og `scrape_options` sendes videre, med hvitelisting. Ukjente nøkler i `scrape_options` avvises. Verdier utenfor området avvises før Quarry.
- Søk: `answerStatus` og `answerReason` går til siden og inn i hurtiglageret.
- Scrape: `max_age` går uendret videre. Testen i `knowledge/quarry.rs` låser det.

Frontend:
- Scrape: «Bruk lagret resultat opptil (minutter)». Resultatet sier når det kom fra et lagret resultat, og hvorfor det ble hentet på nytt.
- Crawl: sitemap-valg, og rensing per side (hovedinnhold, utdragsmodus, base64, inkluder og ekskluder tagger). Standardvalgene sendes ikke.
- Søk: svarstatus med tekst for hver av de ti `answer_reason`-verdiene Quarry kan gi med `unavailable`, og for `no_sources` og `skipped_zdr`. Ukjente årsaker får en generell tekst.
- Kjøring, utdata: «Sider som ble hentet», side for side, med markdown per side.
- Overvåking: «Vis endringen» under siste sjekk og under tidslinjen. Viser lagt til, fjernet og flyttet avsnitt; avsnittsnummer vises fra 1.
- Overvåking: teksten under tidslinjen var utdatert og er skrevet om.
- Feilmeldinger navngir bryterne: `QUARRY_EDGE__SCRAPE_MAX_AGE`, `QUARRY_EDGE__CRAWL_SITEMAP`, `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS`, `QUARRY_EDGE__JOB_RESULTS`, `QUARRY_EDGE__CHANGE_TEXT_DIFF` og `QUARRY_EDGE__ANSWER_SYNTHESIS`. ZDR-tekst for sider og diff.

Ikke verifisert:
- Ingen av flaggene er slått på live, og ingen kall er gjort mot en Quarry med flaggene på.
- Ingen skjermbilder: nettleseren er utlogget. Tomme og feilede tilstander er bare testet i vitest.
- Quarry-punktene 3, 10 og vurderingen av punkt 13 er ikke levert ennå.

Tester: Gateway (bin-suiten, cargo): 849 bestått, 0 feilet. Hele SPA-vitest: 3963 bestått, 1 feilet (BrowserBlockedNotice, filen og importene er ikke endret i denne grenen), 2 testfiler kan ikke kjøres (workspace-pakkene @quarry/client og @verevon/remote-core mangler i worktreet). Typecheck: 33 feil, samme som baseline. Lint på endrede filer: rent. Gateway-kontrakttester (node): 26 bestått. Ingestions- og i18n-suiter: 175 bestått.
