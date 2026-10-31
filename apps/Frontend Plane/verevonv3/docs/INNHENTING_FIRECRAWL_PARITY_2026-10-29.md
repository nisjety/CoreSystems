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
| Extract: strukturert data med JSON-skjema | `POST /v1/extract`, `schema` | Flagg `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` (live: 501). Koblet i Verevon: årsaken og neste steg | `extract_routes.rs`, `errors.ts` |
| Extract: fri prompt | `prompt` | Koblet i Verevon. Quarry-siden bruker den (5f1ac3ca2, ingen flagg) | `extract_routes.rs`, `runs.rs` (`extract_prompt`) |
| Agent: autonom nettleseragent | `/v1/agent/runs` via Verevon | Koblet via `ingestions.start_agent_run` | `dispatchers.rs`, `browser.rs` |
| Agent: direkte Quarry-API uten Verevon | krever nettleser-grant fra gatewayen | Mangler (se under) | `agent_routes/lifecycle.rs` |
| Monitor: sjekk side for endring | `POST /v1/change/check` | Koblet | `change_routes.rs` |
| Monitor: gjentakende overvåker (time, dag, uke) | `POST /v1/schedules` med `target_kind=change_monitor` | Koblet (opprett/stopp) | `monitoring.rs` |
| Monitor: tekstdiff mellom versjoner | `GET /v1/change/diff` | Flagg `QUARRY_EDGE__CHANGE_TEXT_DIFF` (live: av). Koblet i Verevon: «Vis endringen» | `monitoring.rs`, `ChangeDiffPanel.tsx` |
| Monitor: varsel i appen | `change_detected` via control | Koblet (listes i Overvåking) | `notify/` |
| Monitor: varsel på e-post / webhook | `/v1/webhook-subscriptions` | Koblet i Verevon: «Webhooks» under Overvåking. Flagg `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` (og `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS`), av på live | `ingestions/webhooks.rs`, `ChangeWebhooksPanel.tsx` |
| Tidsplaner: cron, pause, utløs | `/v1/schedules` | Koblet; lagring er av på live (se under) | `schedules.rs` |
| Jobber: liste, status, feil per URL, robots-blokkert | `/v1/jobs/{id}`, `/v1/jobs/{id}/errors` | Koblet (ny i denne runden) | `ingestions/jobs.rs` |
| Jobber: opprinnelse (manuell eller tidsplan) | `origin`, `schedule_id` | Koblet (ny i denne runden) | `ingestions/runs.rs` |
| Kjøring: sider som ble hentet (Apify-datasett) | `GET /v1/jobs/{id}/results`, `/v1/crawl/{id}/results`, `/v1/batch/{id}/results` | Flagg `QUARRY_EDGE__JOB_RESULTS` (live: av). Koblet i Verevon: «Sider som ble hentet», med markdown per side | `ingestions/jobs.rs`, `IngestionsRunsView.tsx` |
| Forbruk: kreditter for perioden | `GET /v1/team/credit-usage` | Koblet (ny i denne runden) | `ingestions/jobs.rs` |
| Parse: dokument (PDF, DOCX) til Markdown | `POST /v1/parse`, ingen lagring og ingen sladding | Koblet i Verevon: «Dokumenter», bak flagget `QUARRY_EDGE__PARSE_ROUTE` (live: av) | `ingestions/parse.rs`, `DocumentReadPanel.tsx` |
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
| `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` | AI-formater på scrape (json, summary, question, highlights) og strukturert uttrekk | av: AI-formater gir 400 `feature_disabled`, `/v1/extract` gir 501 (`UNSUPPORTED`, med `next_actions`) |
| `QUARRY_CONTROL__JOB_CREDITS=on` | kreditter per jobb (`credits`), default av | av: feltet mangler i svaret, og siden sier det |
| `QUARRY_EDGE__SCRAPE_MAX_AGE=on` | `max_age` på scrape, med Redis for hurtiglageret | av: 400 `feature_disabled` på `max_age` |
| `QUARRY_EDGE__CRAWL_SITEMAP=on` | `sitemap` på crawl | av: 400 `unsupported_option` på `sitemap` |
| `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS=on` | `scrape_options` på crawl | av: 400 `unsupported_option` på `scrape_options` |
| `QUARRY_EDGE__CHANGE_TEXT_DIFF=on` | `GET /v1/change/diff` og markdown på basislinjer | av: 501 |
| `QUARRY_EDGE__JOB_RESULTS=on` | sider per kjøring | av: 501 |
| `QUARRY_EDGE__ANSWER_SYNTHESIS=on` | svar i søk (`answer_status`) | av: `answer_status` er `unavailable` med `synthesis_disabled` |
| `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS=on` (og `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS=on`) | abonnement på endringswebhook, med UI | av: 501, og meldingen navngir bryteren |
| `QUARRY_EDGE__PARSE_ROUTE=on` | dokumentlesing: PDF og DOCX til Markdown, uten lagring | av: 501 `parse_route_disabled`, og siden navngir bryteren |

Ingen av disse er slått på i denne runden. Å slå dem på er en konfigurasjonsendring
på den delte live-stacken, og det må godkjennes av eieren.

## Mangler i Quarry

Hver linje peker til stedet i Quarry-koden. Nummereringen er den samme som før, så den kan sammenlignes med tidligere versjoner av dette dokumentet. Quarry-punktene fra den andre leveransen står i parentes der de tilsvarer en linje her.

0. **Live-gatewayen er ikke oppdatert.** Den kjører build fra `14299e659`. Endringene fra `b2bf63e2a` og fra denne runden er testet med cargo, wiremock og vitest, ikke på live. Å bytte live krever eierens godkjenning.
1. **AI-formater på scrape (`json`, `summary`, `question`, `highlights`).** Levert på `quarryv2-next` og på lanen `innh/lane-a` (336b1da1b). Begge blokkerne er lukket: `truncated_input` settes over 60 000 tegn, og en personvernblokk (`credential_or_secret`, `zdr_ephemeral`) gir `skipped` med `PRIVACY_POLICY` uten modellkall. Fortsatt av på live: `QUARRY_EDGE__AI_FORMATS` er ikke slått på, og modellplanen er ikke koblet.
2. **Fri `prompt` på `/v1/extract`: levert** (5f1ac3ca2). Prompten brukes nå, også uten skjema.
3. **Tekstdiff for overvåking (Quarry-punkt 4).** Levert på `innh/lane-b` (c9cca3667, 9455099e3, 96c5dc9c3): markdown-artefakt på basislinjen og `GET /v1/change/diff`, bak `QUARRY_EDGE__CHANGE_TEXT_DIFF` (av). SQL-en er ikke prøvd mot Postgres.
4. **Abonnement på endringswebhook (Quarry-punkt 5).** Kontrakten er endelig: sikkerhetsgjennomgangen fant ingen blokkere. Gatewayen og siden er bygget i denne runden (se nederst). Bak `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` og `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS`, begge av. Leveringen er forsøk med nye forsøk. Quarry har ikke bakt inn garantert levering ennå, så siden lover den ikke.
5. **Sitemap og skrape-valg for crawl (Quarry-punkt 6).** Levert på `innh/lane-c` bak to flagg (av). Rulles ut i rekkefølgen i RUNBOOKS: orkestratoren først, så edge-flagget.
6. **Maks alder på scrape (Quarry-punkt 7).** Levert på `innh/lane-a` (31d9ad15f, 5119e6ef8): `max_age` i millisekunder og `cache_state` i svaret, bak `QUARRY_EDGE__SCRAPE_MAX_AGE` (av). Uten Redis gir den `bypass` med advarselen `cache_unavailable`.
7. **Svar i søk (Quarry-punkt 8a).** Levert på `innh/lane-d` (ab65682aa, 0aa5cbbb7): `answer_status` og `answer_reason`. Live svarer `synthesis_disabled` helt til bryteren `QUARRY_EDGE__ANSWER_SYNTHESIS` er slått på. Det er en konfigurasjon, ikke kode.
8. **Kostnad per kjøring.** Levert på `quarryv2-next` (6bbd99763), bak `QUARRY_CONTROL__JOB_CREDITS` (av).
9. **Direkte agent-API for Quarry-kallere.** `/v1/agent/runs` krever `grant_id` fra gatewayen (`agent_routes/lifecycle.rs`), så en ekstern klient kan ikke starte en agent direkte. Quarry-punkt 10 (agentkjøringer med API-nøkkel) er bygget på Quarry-siden bak `QUARRY_EDGE__API_KEY_AGENT_RUNS` (av). Gatewayen mangler sin halvdel, og Auth Core mangler utstedelse av nøkler. Begge venter på eierens beslutninger OD-1 til OD-17. Ingenting å koble i Verevon ennå.
10. **Parse av dokumenter til Markdown (Quarry-punkt 11).** Levert på `innh/integ` (b68c347e9, a994e3cd7), lokalt og ikke pushet, bak `QUARRY_EDGE__PARSE_ROUTE` (av: 501 `parse_route_disabled`). Kontrakten er i `docs/CONTRACTS.md` under `/v1/parse`. Råbytes går opp med PDF- eller DOCX-typen, og svaret er Markdown med advarsler (skann, sider uten tekstlag, tomt dokument, kappet ved 8 MiB). Ingen lagring, ingen sladding (personopplysninger kommer tilbake uendret) og ingen måling. Koblet i Verevon: fanen «Dokumenter». Ikke kjørt på stack: Docker var nede hos Quarry-sesjonen.
11. **Resultater per side for en jobb (Quarry-punkt 12).** Levert på `innh/lane-b` (cc2596c7a, 4f2a1bc3c): `GET /v1/jobs/{id}/results`, `/v1/crawl/{id}/results` og `/v1/batch/{id}/results`, bak `QUARRY_EDGE__JOB_RESULTS` (av), og `GET /v1/artifacts?run_id=&kind=`. Ikke prøvd mot et stack.
12. **Schedule-kinds på live.** `QUARRY_SCHEDULE_CAPABILITIES` er tom, så siden kan ikke lagre tidsplaner. Må: konfigurasjon, etter at utførelsen er verifisert.
13. **Vurdering av Quarry-punkt 13 (avbryt, nytt forsøk, eksport, kjøringsferdig-webhook).** Anbefalt rekkefølge: avbryt først (bygget bak `QUARRY_EDGE__JOB_CONTROL`, ikke prøvd på stack), så kjøringsferdig-webhook (1 til 1,5 dager, bare en ny hendelse i listen), så JSON-eksport hvis eieren vil ha den (paginert JSON og resultatruten finnes), og til slutt nytt forsøk (bare agentkjøringer kan gjenopptas i dag). Ingen kode i Verevon for dette ennå.

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

## Endringer i runden etter (tredje leveranse fra Quarry)

Gateway (`apps/gateway`):
- `/api/ingestions/webhooks` (GET, POST), `/api/ingestions/webhooks/{id}` (PATCH, DELETE), `…/rotate-secret` (POST), `…/deliveries` (GET) og `…/deliveries/{delivery_id}/retry` (POST) går til `/v1/webhook-subscriptions`. Hendelsen er fast `change_detected`. Svarene som bærer hemmeligheten (opprett og roter) får `Cache-Control: no-store`. PATCH slipper bare `active` og `description`.

Frontend:
- Overvåking: «Webhooks» under varslene. Opprett (https-adresse, valgfri beskrivelse), pause og aktiver, roter hemmelighet og slett (begge med bekreftelse), og leveringslogg med «Send på nytt» for feilede og gitte opp leveringer.
- Hemmeligheten vises én gang, i minnet, og lagres ikke i nettleseren. Skjemaet skjules for ZDR-organisasjoner. Lesing, pause, rotasjon og sletting er åpne for dem.
- Feilmeldinger: bryteren som er av, ZDR, grensen på 10, duplikat og hver feltgrunn fra Quarry (`docs/ERROR_REASONS.md`).
- Strukturert uttrekk: 501 viser hvilken av de fire årsakene som gjelder (Quarry skiller dem bare på teksten i `error`), og neste steg: kjør uten skjema og prompt for Markdown. `hint` vises ikke, fordi den er tekst til operatørene.

Tester: gateway (bin-suiten): 856 bestått, 7 nye. Ingestions og shared/api i vitest: 471 bestått; `quarry-client.test.ts` kan ikke kjøres, som før. Panelets tre atferdstester: bestått. Typecheck: 33 feil, samme som baseline. ESLint på endrede filer: rent.

Ikke verifisert:
- Ingen webhook-bryter er slått på, og ingen kall er gjort mot en Quarry med dem på.
- Leveringsstatusene `pending`, `in_flight`, `success`, `failed` og `dlq` er bekreftet mot koden på `innh/integ` av Quarry-sesjonen, ikke på et stack. `in_flight` kan stå der uten at API-et kan sende den på nytt, så siden viser den som «sendes» uten knapp. «Send på nytt» vises bare for `failed` og `dlq`.
- Ingen skjermbilde av webhook-panelet: nettleseren er utlogget.

## Endringer i runden etter (fjerde leveranse fra Quarry)

Quarry-punkt 11, `POST /v1/parse`, er levert på `innh/integ` (b68c347e9, a994e3cd7), lokalt og ikke pushet. Kontrakten står i `docs/CONTRACTS.md` under `/v1/parse`.

Gateway (`apps/gateway`):
- `POST /api/ingestions/parse` sender råbytes videre med klientens `Content-Type`, og Quarry avgjør typen fra innholdet. Bare `Content-Type` går videre, ikke filnavnet.
- Gatewayen avviser en fil over 25 MiB før noe leses når lengden er oppgitt, og ellers ved grensen. Avvisningen er standardkonvolutten med `details.max_bytes`, samme som Quarry gir.
- Svaret får `Cache-Control: no-store`, og `Retry-After` fra Quarry sendes videre. Dokumentet logges ikke. Tidsgrensen er 60 s: Quarry svarer innen sitt eget mål på 15 s, og opplastingen skal også få tid.

Frontend:
- Ny fane «Dokumenter», etter «Lekeplass». Velg en PDF eller DOCX og få Markdown. Advarslene står over teksten. Type, størrelse og antall sider vises.
- En fil som ikke er PDF eller DOCX, eller er over 25 MiB, avvises før noe sendes.
- 501 sier at dokumentlesing ikke er slått på, og navngir `QUARRY_EDGE__PARSE_ROUTE` når Quarry sier det. 429 gir en nedtelling fra `Retry-After`, og knappen kan trykkes først når den er ute.
- Ingen kostnad vises: ruten er ikke målt. Siden sier at ingenting lagres, og at personopplysninger ikke fjernes.

Ikke verifisert:
- Ruten er ikke kjørt på et stack: Docker var nede hos Quarry-sesjonen. Bryteren er av på live.
- Ingen kall mot Quarry med bryteren på, og gatewayen er ikke testet mot en ekte Quarry. Testene dekker grensene og konvolutten, ikke videresendingen av bytes.
- Ingen skjermbilde av fanen: nettleseren er utlogget.

Tester: gateway (bin-suiten): 861 bestått, 0 feilet, 5 nye for parse. Ingestions og shared/api i vitest: 485 bestått; `quarry-client.test.ts` kan ikke kjøres, som før. De tre nye testfilene for dokumentfanen: 14 bestått. Typecheck: 33 feil, samme som baseline. ESLint på endrede filer: rent.
