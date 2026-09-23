# Datakilder — hva er ekte og hva er mock

> **Historisk kildeaudit, oppdatert med status 23. september 2026:** Tabellen nedenfor beskriver kjøringen 14. september. [Q05–Q07](../../../../docs/PRODUCT_RECORDING_Q05_Q07_2026-09-19.md) verifiserte senere miljø-/tidsstempel-/kolliproveniens på 13 ett-kolli-tilbud (Bring/UPS produksjonskonfigurert, DHL sandbox); FedEx manglet Rates API-tilgang da. Dette er ikke et tilbud for hybridordren. Den testarbeidsflaten som ble undersøkt da hadde ingen Visma MCP-registrering, ingen indekserte dokumenter og ingen innbokssamtaler; dagens integrasjonstilstand må kontrolleres på nytt. ERP-data limt inn av en annen klient beviser fortsatt ikke ERP-oppslag fra Verevon. [Q08–Q10](../../../../docs/PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) dokumenterer det senere hybridarbeidet, mens [pass 6](../../../../docs/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) er gjeldende status for de separate produktfilmene med fiktive data.

Laget 14. september 2026 for prøvekjøringen med hybrid datagrunnlag.
**Dette dokumentet skal ikke legges ved som AI-grunnlag.** Det er sporet over hva som må erstattes med ekte kilder før en reell demo.

## Kort oppsummering

| Kilde | Status i denne kjøringen | Hentet av |
| --- | --- | --- |
| Visma.net salgsordrer | **EKTE** | Claude Code via Visma Net MCP, lagt inn i kildepakkene som tekst |
| Frakt (shipping-core) | **MOCK i praksis** — se funn F1 | Verevon selv, verktøyet `shipping_get_quotes` |
| E-post | **EKTE innboks finnes**, men ikke brukt som grunnlag | — |
| Kundehenvendelser, møtenotat, brief, rutiner | **MOCK** | skrevet for denne prøven |

## Per oppgave

### 01 Kundesvar
**EKTE (Visma.net, ordre I1/12475, hentet 14.09.2026):**
- Ordrenummer, dato, kunde «Åkeberg Skoglunn Pølsemakeri AS» (kundenr. 305097)
- Status «Hold», ikke kansellert
- Ordretotal 18 225,89 NOK
- Alle fem ordrelinjer: varenummer, produktnavn, antall, enhet og enhetspris

**LIVE, men ikke ekte data (se funn F1):**
- Fraktestimatet ble hentet live av Verevon via `shipping_get_quotes` mot shipping-core. Selve kallet er ekte integrasjon; prisene som kom tilbake var fra mock-transportører (`mock-dsv`, `mock-postnord`). Booking er uansett av (`BRING_LIVE_BOOKING` er tom).

**MOCK:**
- Selve kundehenvendelsen (K1). Det finnes ingen ekte e-post fra denne kunden om denne ordren.
- Leverings- og avsenderadresse, vekt og antall kolli (K3). Visma-uttrekket vi brukte har ikke `shipping`-feltene, så dette er satt for å kunne be om et fraktestimat.
- Rutinen for ordre på «Hold» (K4). Aquatiqs virkelige rutine ligger ikke i kunnskapsbasen.
- Begrunnelsen for hvorfor ordren står på «Hold» (ikke oppgitt i Visma-uttrekket).

**For å gjøre ekte senere:** hent leveringsadresse og vekt fra Visma-ordren (`shipping`-objektet, andre feltnavn enn vi prøvde), legg den virkelige Hold-rutinen i kunnskapsbasen, og bruk en reell kundehenvendelse fra innboksen.

### 02 Salgsrapport
**EKTE (Visma.net, uttrekk 14.09.2026, 1 000 nyeste ordrer sortert på dato):**
- Uke 36: 175 ordrer, 6 936 786,20 NOK
- Uke 37: 217 ordrer, 7 410 798,41 NOK
- Statusfordeling per uke (Completed/Open/Invoiced/Shipping/BackOrder/Hold/Cancelled)
- Begge ukene er komplette i uttrekket (det rakk tilbake til 5. august)

**MOCK:**
- Salgssjefens merknad (S4) — formuleringene om mulige årsaker er skrevet for prøven.

**UTELATT med vilje (verken ekte eller mock):**
- Bruttofortjeneste og margin. `unitCost` finnes per ordrelinje i Visma, men å hente linjer for alle 392 ordrer krever mange sidevise kall, og 22 av 179 linjer i stikkprøven manglet `unitCost`. Kildepakken sier derfor eksplisitt at margin ikke kan beregnes.
- Produktgruppe- og kundefordeling. Krever samme linjeuttrekk.

**For å gjøre ekte senere:** page gjennom ordrelinjene for de to ukene og aggreger `quantity * unitCost` mot `orderTotal`. Da blir margin, produktgruppe og topp kunder ekte også.

### 03 Kampanje
**EKTE (Visma.net):**
- Produktnavn, varenummer, emballasjestørrelse og enhetspris for fem produkter (M2) — hentet fra virkelige ordrelinjer
- Kundetypene i M3 er virkelige kundenavn fra ordrer i uke 36–37

**MOCK:**
- Hele kampanjebriefen (M1): periode, målgruppebeskrivelse, formål, format, oppfordring
- Språkprofilen (M4)
- De ikke godkjente påstandene (M5)

**For å gjøre ekte senere:** legg de virkelige produktdatabladene i kunnskapsbasen. Uten dem kan tekstene bare bruke navn, størrelse og pris — ikke bruksområde, dosering eller godkjenninger.

### 04 Prosjektplan
**EKTE:** ingenting utover demodatoen.

**MOCK:** hele møtenotatet, rollene, rammene og ferdigkriteriene. Kundeportalpiloten er oppdiktet.

**For å gjøre ekte senere:** bruk et virkelig møtereferat (Teams-transkripsjon eller et prosjektdokument). Verevon har ingen møte- eller kalenderkilde i dag, så dette er den oppgaven som er lengst fra ekte data.

## Hva Verevon selv kunne nå under kjøringen

Verktøy modellen faktisk har (fra `builtin_tool_defs`, 20 stk): `shipping_get_quotes`, `knowledge_search`, `knowledge_list_documents`, `knowledge_graph_search`, `knowledge_wiki_search`, `knowledge_contradictions`, `web_search`, `fetch_url`, `get_weather`, `code_interpreter`, `create_artifact`, `read_artifact`, `update_artifact`, `result_query`, `reattach_context`, `reattach_skill`, `insights_overview`, `social_list_accounts`, `social_list_posts`, `social_list_campaigns`.

Rettelse: `create_order` er **ikke** et ekte verktøy. Det er en testfixture i en enhetstest i `tool_loop.rs` (MCP-server `mcp.example.test`), og ble feilaktig tatt med i en tidligere versjon av denne listen. Verevon kan altså ikke opprette ordrer i dag.

**Mangler for at dette skal bli ekte ende-til-ende:**
1. **Visma:** ingen ERP-verktøy i modellens verktøyliste. MCP-veien finnes (`/api/v1/mcp/servers`), men returnerte 503 til vi rettet det i dag (se funn 30). Etter at serveren er koblet til, må noen godkjenne OAuth i grensesnittet — det kan ikke gjøres på vegne av brukeren.
2. **E-post:** ingen innboks-verktøy for modellen i det hele tatt. Innboksen finnes i produktet (`/api/v1/inbox/...`) og synkroniserer ekte e-post, men modellen kan ikke lese den under en samtale.
3. **Frakt:** integrasjonen virker ende-til-ende, men prisene er ikke ekte. Se funn F1 — mock-transportører ligger fortsatt i fleeten, og de ekte adapterne peker på test-/sandkassemiljøer.


---

# Resultat av prøvekjøringen 14.09.2026

Fire samtaler, ett vedlegg hver (`kildepakke.md`), prompt fra `prompt.txt`.

| Oppgave | Tråd | Utfall |
| --- | --- | --- |
| 01 Kundesvar | `01M2GV8A3XESYPA7B5THK45NQY` | Utkast levert med ekte ordredata + fraktestimat. Første forsøk (`01M2GV5XCFSCK59SFY3RTZBQYQ`) stoppet — se funn F2. |
| 02 Salgsrapport | `01M2GVD3M17FDBZXKGTMD118VV` | Rapport levert. Alle tall stemmer med Visma-uttrekket. |
| 03 Kampanje | `01M2GVHFF3QQ4F8BR3MAAVGZVC` | Tre innlegg + e-post levert med ekte produktnavn. Se funn F3. |
| 04 Prosjektplan | `01M2GVN5B9D9F19JBASR7HVY8W` | Plan levert med T1–T7, riktig rekkefølge og scenario for feilretting. |

## Funn fra kjøringen

**F1 — Frakt er ikke ekte, selv om oppstartsloggen sier «real adapter».**
Verevon fikk svar fra `shipping_get_quotes`, men det billigste tilbudet kom fra `carrier_code: "mock-dsv"`. shipping-core bygger fleeten som mock-adaptere og bytter ut den enkelte transportøren *bare* når legitimasjon finnes. Bring, DHL, UPS og FedEx logget «using real adapter» ved oppstart, men:
- DHL peker på `https://express.api.dhl.com/mydhlapi/test` (testmiljø)
- UPS peker på `https://wwwcie.ups.com` (testmiljø)
- FedEx peker på `https://apis-sandbox.fedex.com` (sandkasse)
- DSV og PostNord har ingen ekte adapter i det hele tatt og blir liggende igjen som mock

Konsekvensen er at sammenligningstabellen blander sandkassepriser og ren mock, og at den *billigste* — altså den kunden blir presentert for — kan være fullstendig oppdiktet. I denne kjøringen ble kunden tilbudt «DSV Standard ca. kr 669» og «PostNord kr 808», begge mock.
**Må rettes før ekte demo:** enten produksjonsnøkler for minst én transportør og fjerning av mock-adapterne, eller en tydelig merking i svaret om at prisene er estimater fra testmiljø.

**F2 — Fraktverktøyet krever at alle argumenter finnes ordrett i samtalen.**
Første forsøk på oppgave 01 feilet: modellen fant på avsendernavn og kollidimensjoner, og grunnlagssjekken stoppet kallet («It supplied values that appear nowhere in this conversation»). Modellen spurte brukeren om de to verdiene i stedet for å gjette — riktig oppførsel. Kildepakken måtte utvides med eksakte verdier for `from`, `to`, `package` og `segment`.
**Ikke en feil**, men verdt å vite: en fraktforespørsel krever at adresse, vekt og mål står i grunnlaget.

**F3 — Kampanjeutkastene brukte påstander kildepakken forbød.**
M2 sier uttrykkelig at det ikke finnes produktdatablad, og at bruksområde og effekt ikke skal påstås. Utkastene skrev likevel «alkaliske rengjøringsmidler til CIP-anlegg», «tilpasset industriell bruk» og «i daglig bruk hos produsenter i norsk sjømat og meieri». Det siste kobler M3s kundeliste til bestemte produkter, noe grunnlaget ikke viser.
**Må rettes før ekte demo:** legg de virkelige produktdatabladene i kunnskapsbasen, så blir påstandene dekket — eller stram inn prompten.

**F4 — Kunnskapsbasens to aquatiq.com-dokumenter er tomme.**
Kunnskapssøket i oppgave 03 traff dokument `6bef270c-…` som bare inneholder «Redirecting (308) … The document has moved». De to web-sidene i kunnskapsbasen er 63 og 84 tegn og har ingen brukbar informasjon.
**Må rettes:** kjør en ny crawl av aquatiq.com som følger omdirigeringen.

**F5 — Ordreoppslaget i Visma manglet fraktfeltene vi prøvde.**
`shipping { shipVia shipTerms shipDate shipToAddress }` finnes ikke på `SalesOrderShipping` i dette skjemaet. Leveringsadresse og vekt i K3 er derfor satt manuelt. Feltnavnene må slås opp med `introspect` før en ekte kobling.
