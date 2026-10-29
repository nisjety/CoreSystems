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
| Scrape: maks alder / cache | cache-policy finnes i Quarry, men scrape-endepunktet tar ikke imot `maxAge` i dag | Mangler (se under) | `quarry-core/src/cache.rs` |
| Scrape: JSON-format med skjema per side | `formats: json` | Mangler (reservert) | `quarry-core/src/scrape.rs:306` |
| Scrape: sammendrag, spørsmål, høydepunkter | `formats: summary / question / highlights` | Mangler (reservert) | samme |
| Scrape: branding (farger, logo) | feltet `branding` finnes i svaret | Koblet (vises i JSON og metadata) | `pipeline.rs` |
| Scrape: lagre i Kunnskap | `ingest: true` | Koblet ("Send til Kunnskap") | `knowledge/quarry.rs` |
| Map: finn URL-er på et nettsted | `POST /v1/map` | Koblet | `knowledge/quarry.rs` (`crawl/discover`) |
| Search: nettsøk | `POST /v1/search` | Koblet | `search.rs` |
| Search: tidsrom, emne, domener, eksakt treff | `time_range`, `topic`, `include_domains`, `exclude_domains`, `exact_match` | Koblet | `search.rs` |
| Search: marked og språk (Quarry) | `country`, `language` | Koblet (lagt til i denne runden) | `search.rs` |
| Search: svar generert fra treffene | `include_answer` | Mangler svarleverandør (se under) | `answer_routes.rs` |
| Search: skrape alle treff | `POST /v1/batch` med treffene | Koblet | |
| Crawl: følg lenker, dybde, stier, underdomener | `max_pages`, `max_depth`, `include_paths`, `exclude_paths`, `allow_subdomains` | Koblet | `crawl_request.rs` |
| Crawl: sitemap | `sitemap` (kun `skip` støttes) | Mangler | `crawl_request.rs:56` |
| Crawl: skrape-valg per side | `scrape_options` | Mangler | `crawl_request.rs:61` |
| Crawl: pause, gjenoppta, avbryt | `POST /v1/crawl/{id}/pause` m.fl. | Flagg `QUARRY_EDGE__JOB_CONTROL` | `job_lifecycle_routes.rs` |
| Batch: liste med URL-er | `POST /v1/batch` | Koblet, også pause/avbryt | |
| Extract: strukturert data med JSON-skjema | `POST /v1/extract`, `schema` | Flagg `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` (live: 501 med hint) | `extract_routes.rs` |
| Extract: fri prompt | `prompt` | Mangler (lest, men ikke brukt) | `extract_routes.rs:48` |
| Agent: autonom nettleseragent | `/v1/agent/runs` via Verevon | Koblet via `ingestions.start_agent_run` | `dispatchers.rs`, `browser.rs` |
| Agent: direkte Quarry-API uten Verevon | krever nettleser-grant fra gatewayen | Mangler (se under) | `agent_routes/lifecycle.rs` |
| Monitor: sjekk side for endring | `POST /v1/change/check` | Koblet | `change_routes.rs` |
| Monitor: gjentakende overvåker (time, dag, uke) | `POST /v1/schedules` med `target_kind=change_monitor` | Koblet (opprett/stopp) | `monitoring.rs` |
| Monitor: tekstdiff mellom versjoner | ingen tekst lagres, bare fingeravtrykk | Mangler (se under) | `change_routes.rs` |
| Monitor: varsel i appen | `change_detected` via control | Koblet (listes i Overvåking) | `notify/` |
| Monitor: varsel på e-post / webhook | webhook sendes til control, men ingen abonnements-API | Mangler (se under) | `change_webhook.rs` |
| Tidsplaner: cron, pause, utløs | `/v1/schedules` | Koblet; lagring er av på live (se under) | `schedules.rs` |
| Jobber: liste, status, feil per URL, robots-blokkert | `/v1/jobs/{id}`, `/v1/jobs/{id}/errors` | Koblet (ny i denne runden) | `ingestions/jobs.rs` |
| Jobber: opprinnelse (manuell eller tidsplan) | `origin`, `schedule_id` | Koblet (ny i denne runden) | `ingestions/runs.rs` |
| Jobber: kostnad per kjøring | jobbene har ingen kostnadsfelt | Mangler (se under) | |
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
| `QUARRY_EDGE__AI_FORMATS=1` + `MODEL_PLANE_URL` | strukturert uttrekk med JSON-skjema | av: `/v1/extract` svarer 501 med hint |

Ingen av disse er slått på i denne runden. Å slå dem på er en konfigurasjonsendring
på den delte live-stacken, og det må godkjennes av eieren.

## Mangler i Quarry

Hver linje peker til stedet i Quarry-koden, slik at arbeidet kan starte der.

0. **Live-kjøring er ikke oppdatert.** Den kjørende gateway-containeren (`verevon-gateway-rs`) kjører fortsatt bygget fra `main`, så de nye rutene (`/api/ingestions/jobs`, `/usage`, action-kontraktene) finnes ikke der før containeren byttes. Verifisert i en sidecontainer fra den nye imagen (`verevon-gateway-innh`, port 3186).
1. **Scrape-formatene `json`, `summary`, `question`, `highlights`.** Reservert i
   `quarry-core/src/scrape.rs:306` (`RESERVED`), og avvist med `reserved_format`.
   Må: fjerne fra reservert liste, koble `AiFormatRunner` (`quarry-runtime/src/ai_formats.rs`)
   til scrape-svaret, og legge til kontrakttester. Krever modellplanen.
2. **Fri `prompt` på `/v1/extract`.** Feltet leses, men er markert `dead_code`
   (`extract_routes.rs:48`): "folded into schema-guided prompt in follow-up".
   Må: bruke prompten i den skjemastyrte forespørselen til modellen.
3. **Tekstdiff for overvåking.** `record_check` lagrer `artifact_id: None` på
   basislinjen (`change_routes.rs`), og diff-artefakten er bare fingeravtrykk-overgangen.
   Må: lagre markdown-artefakten for hver ny eller endret basislinje, sette
   `artifact_id`, og ha et endepunkt som gir to versjoner å sammenligne.
4. **Abonnement på endringswebhook.** `change_webhook.rs` sender
   `quarry.change.detected` til control, men gatewayen har ingen rute for å
   opprette, liste eller slette abonnement, og SPA-en har ingen UI for det.
   Må: abonnements-API i control, rute i gatewayen, og en UI-flate.
5. **Sitemap og skrape-valg for crawl.** `crawl_request.rs:56` og `:61` avviser
   dem med `unsupported_option`. Må: implementere sitemap-oppdagelse og
   `scrape_options` per side.
6. **Maks alder (cache) på scrape.** Cache-policyen finnes (`quarry-core/src/cache.rs`),
   men scrape-svaret viser bare `cache_state: bypass` og tar ikke imot `maxAge`.
   Må: godta `max_age` på scrape og respektere den.
7. **Svar i søk.** `include_answer` gir `answer: null` på live, og `/v1/answer`
   svarer 501 når ingen svar-pipeline er satt opp (`answer_routes.rs`).
   Må: konfigurere en svarleverandør på edge. Dette er en konfigurasjon, ikke kode.
8. **Kostnad per kjøring.** `JobSummary` og `JobStatusView` har ingen kredittfelt,
   og `/v1/team/credit-usage` er bare summert for teamet. Må: skrive kreditter per
   jobb i jobbposten (eller hendelsene) og eksponere dem.
9. **Direkte agent-API for Quarry-kallere.** `/v1/agent/runs` krever `grant_id`
   fra gatewayen (`agent_routes/lifecycle.rs`), så en ekstern klient kan ikke starte
   en agent direkte. Må: en grant-utstedelse som kan brukes av en API-nøkkel.
10. **Parse av dokumenter til Markdown.** Pipeline-en leser PDF og DOCX
    (`pipeline.rs:1043`), men bare som opptak til kunnskapsbasen. Må: en offentlig
    rute som returnerer Markdown uten å lagre.
11. **Resultater per side for en jobb.** Det finnes ingen rute som gir sidene en
    crawl eller batch hentet. Feil og hendelser finnes, og sidene havner i
    Kunnskap når `ingest` er på. `ListFilter` for artefakter har heller ikke
    `run_id`. Må: filtrer artefakter på kjørings-ID, eller en resultatrute per jobb.
12. **Schedule-kinds på live.** `QUARRY_SCHEDULE_CAPABILITIES` er tom, så
    siden kan ikke lagre tidsplaner. Må: konfigurasjon, etter at utførelsen er verifisert.

## Endringer i denne runden

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
