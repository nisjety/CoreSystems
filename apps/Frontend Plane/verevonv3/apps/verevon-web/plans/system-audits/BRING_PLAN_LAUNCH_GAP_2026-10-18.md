# Bring / Plan — gap mellom nettstedets scene og lanseringsklar flyt

Dato: 2026-10-18. Dette er en kildekode- og dokumentgjennomgang, ikke en live-test eller en sertifisering av deployet miljø. Statusene nedenfor skiller eksplisitt mellom gjenbrukbare byggesteiner og den sammenhengende arbeidsflyten som nettstedet lover.

## Løftet vi må bevise

Scene 02 sier at Bring melder forsinkelse, at Verevon samler avtale, e-post og leveringsstatus, vurderer konsekvensen for monteringen og forbereder oppfølgingen; mennesket tar stilling til det som skal sendes (`src/components/home/sections/ProductLoopSection.tsx`). Lanseringskravet i `plans/launch-promises.md` er en **ekte hendelsesdrevet sak** med sporbare kilder og beslutninger, der bare den utgående kommunikasjonen som krever beslutning stopper for godkjenning. Filmen er ikke bevis for dette.

## Funn

| Ledd | Det som finnes i kildekoden | Gap mot lanseringsløftet | Vurdering |
| --- | --- | --- | --- |
| Bring-signal | Ingestion `shipping-core` har autentisert, tenant-avgrenset tracking-oppslag, samt periodisk polling av *egne bookede* sendinger (`internal/booking/service.go`, `cmd/shipping-core/main.go`). Model `execution-core` kan slå opp en sending med sporingsnummer og organisasjon (`shipping_tools.rs`). | Ved polling persisteres tracking-hendelser, men sideeffekt/observer utløses bare ved **levert**, ikke ved **forsinket**. Den publiserte hendelsen i dette løpet er `booking.delivered`; ingen dokumentert `shipment.delayed` → sak-kjede ble funnet. En ekstern Bring-sending som ikke tilhører organisasjonens bookingliste er heller ikke en automatisk hendelseskilde her. | Byggestein; ingen ende-til-ende forsinkelsesinngang bevist. |
| Kontekstuell start | Produktets vanlige tomme chat viser generelle, statiske hurtigprompter (`EmptyChatState` i `src/features/chat/components/ChatMessages.tsx`). | «Bring melder …» kommer fra `src/demo/fixtures/fjordform.ts` og injiseres av `src/demo/suggestions.ts` bare i scenemodus. Den produseres ikke fra en virkelig varsel-/sakstrøm i ordinær chat. Scenen krever dessuten at brukeren klikker forslag og sender spørsmålet. | Kun scene for denne hendelsen. |
| Kilder og konsekvens | Den virkelige chatten har SSE-hendelser, kildevisning, artefakter og arbeidstrinn; Data Plane eier retrieval og kildegrunnlag. | Scene 02 svarer på `/api/*` med en fetch-interceptor og fiktive fixtures (`src/demo/README.md`, `fixtures/fjordform.ts`). Bring-status, kundeavtale, Nordlys-e-post og påstanden «Montering 12. okt må flyttes» er hardkodet i scenen. Ingen testet kobling fra faktisk trackinghendelse til riktig avtale, e-post og monteringsplan ble funnet. | UI-kapasitet finnes; scenens sammensatte evidens er simulert. |
| Plan og beslutning | Chat har API-klient for å liste, avgjøre og gjenoppta agentgodkjenninger (`src/shared/api/orchestration-client.ts`; `use-chat-controller.ts`). Model Plane har agent-/policy-byggesteiner. | Scenens `nordlysPlanRows` og `nordlysApproval` er fixtures. Godkjenningen heter «Forbered oppfølging» og oppsummerer tre steg; filmen viser ikke en nøyaktig, tenant-bundet beslutning om **den konkrete kundemeldingen** med mottaker, innhold, datagrunnlag og handlingshash. Ingen filmhandling utfører eller verifiserer sending. | Generisk godkjenningsflate, ikke bevist risikobasert Bring-beslutning. |
| Utgående effekt og revisjon | Flere tjenester har generelle kontrakter for utgående handlinger og audit. Shipping har egen bookingaudit. | Det finnes ikke et verifisert Bring-saksløp som fortsetter sikre interne steg, stopper kundemeldingen før effekt, håndterer avslag/utløp og kvitterer én faktisk sending uten dublett. `MODEL_PLANE_STATUS.md` sier selv at godkjenningsfortsettelse mangler restartbar descriptor/dispatcher/utførelseskvittering i den vurderte kilden; nyere produktflyt må testes mot gjeldende kode og deploy før en slik påstand kan oppgraderes. | Ikke lanseringsbevist. |

### Hvorfor filmen ikke teller som implementasjon

`src/demo/README.md` beskriver en utviklingsmodus som ikke inngår i vanlig build: den avskjærer `/api/*` og spiller inn fiktive SSE-svar over det reelle chatgrensesnittet. `public/product-scenes/SOURCES.md` merker 02 Plan som en illustrasjon med fiktive Fjordform-data. Dette er riktig for markedsføringsfilm, men må ikke brukes som produksjonsakseptanse.

### Kilder og avgrensninger

- Nettsted: `src/components/home/sections/ProductLoopSection.tsx`, `plans/launch-promises.md`.
- Scene: `../../src/demo/README.md`, `../../src/demo/fixtures/fjordform.ts`, `../../src/demo/scenes/02-plan.ts`, `public/product-scenes/SOURCES.md`.
- Faktisk shipping: `apps/Ingestion Plane/shipping-core/internal/booking/service.go`, `cmd/shipping-core/main.go`, `internal/carrier/bring/booking.go`; `apps/Model Plane/rust/services/execution-core/src/shipping_tools.rs`; `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/shipping.rs`.
- Produktgrensesnitt: `apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx`, `use-chat-controller.ts`, `src/shared/api/orchestration-client.ts`.
- Tidligere statusdokumenter er daterte: `apps/Ingestion Plane/INGESTION_PLANE_STATUS.md`, `apps/Model Plane/MODEL_PLANE_STATUS.md`, `plans/system-audits/PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md`. De er ikke bevis for nåværende produksjonsstatus.

## Anbefalt neste implementasjonsrekkefølge

1. **Avgrens en virkelig Bring-pilot.** Én organisasjon, én tenant-eid sending med sporingsnummer, én ordre/avtale, én e-posttråd og en kjent monteringsdato. Avtal hvilke systemer som faktisk er kildene og hvilke tilgangs-/lagringsregler som gjelder.
2. **Gjør forsinkelse til en varig, idempotent hendelse.** Ingestion normaliserer endret ETA/forsinkelsesstatus med leverandør-ID, org, kildeversjon og korrelasjonsnøkkel. En forbruker oppretter/oppdaterer én sak og et relevant, kilde-merket forslag uten at brukeren først må skrive prompt. Dublett, out-of-order og manglende Bring-tilgang skal ha synlig, sikker utgang.
3. **Bygg sammensatt evidens og plan.** Data/Model kobler korrekt avtale, e-post og monteringsplan med per-påstand-kilder. Er én kilde utilgjengelig, skal konsekvensen markeres som usikker, ikke presenteres som faktum.
4. **Skill rutine fra beslutning.** Lesing, sammenstilling og internt planutkast kan gå innenfor policy. En effektfull utgående kundemelding får separat godkjenning som viser mottaker, nøyaktig tekst, hvorfor den foreslås, kilder og konsekvens. Avslag, utløp eller endret mandat må stoppe akkurat denne handlingen.
5. **Kvitter hele løpet.** Send gjennom autorisert provider med engangs-/idempotensnøkkel; vis provider-kvittering, feil og retry uten dobbelsending. Knyt hendelse → sak → kilder → plan → beslutning → effekt i ett revisjonsspor.
6. **Kjør lanseringsbevis.** Automatisert integrasjons-/E2E-test mot tilkoblet testmiljø for suksess, dublett, foreldet ETA, kildemangel, avslag og provider-feil. Deretter verifiser deployet revisjon og tenant-/ZDR-grenser. Før dette er Scene 02 en målillustrasjon, ikke en ferdig produktpåstand.

## Beslutning

**Ikke godkjenn Bring / Plan som lanseringsbevis ennå.** Behold scenen som illustrasjon mens den ekte hendelses-til-effekt-kjeden bygges og måles. Første tekniske milepæl bør være en tenant-bundet `Bring-forsinkelse → én sak → ett kontekstuelt forslag` uten kundesending; da kan resten legges på samme korrelasjons-ID og testes trinnvis.
