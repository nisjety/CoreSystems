# Veien til en ekte ende-til-ende-kjøring

> **Status 23. september 2026:** Dette er en egen plan for en senere demo med virkelige, tilkoblede datakilder. Punktene nedenfor er funn og forslag fra 14. september, ikke en ny verifikasjon av dagens integrasjoner. Den vedlikeholdte [pakken med fiktive opptaksdata](../product-recordings/README.md) er fortsatt blokkert for publisering: fire private prøvefilmer finnes nå, også en prosjektplan med komplett bestått kjøring, men ingen oppgave har fem sammenhengende kvalifiserte publiseringskjøringer. Se [pass 6](../../../../docs/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md). Ikke bruk innlimte ERP-data, mock-frakt eller dette eldre notatet som bevis på at Verevon selv hentet data fra integrasjonene. Før en hybridfilm planlegges må tilkobling, tilgang, kildeproveniens, personvern og alle fire ende-til-ende-forløp verifiseres på nytt med ChatGPT Terra-abonnementsruten.

Mål: de fire oppgavene kjører i Verevon på ekte data, uten mock og uten feilede steg, klart til opptak.
Historisk status per 14. september 2026, etter prøvekjøringen dokumentert i `DATAKILDER.md`.

Hvert punkt har: hva som er galt, hva som skal gjøres, hvem som kan gjøre det, og hvordan vi ser at det er ferdig.
«Bruker» betyr at det krever legitimasjon, innlogging eller en beslutning som ikke kan tas på andres vegne.

---

## Blokk A — Integrasjoner Verevon selv må kunne nå

Uten disse må data limes inn manuelt, og da er det ikke en ende-til-ende-demo.

### A1. Koble Visma Net som MCP-server *(bruker + utvikler)*
- **Nå:** Verevon har ingen ERP-verktøy. MCP-veien var nede (503) til vi rettet den i dag; den svarer nå 200.
- **Gjør:**
  1. Finn URL-en til Visma Net MCP-serveren.
  2. Kall `POST /api/v1/mcp/servers/oauth/start` med `name`, `url`, `tool_allowlist`, `scope`. Verevon registrerer klienten dynamisk (RFC 7591) — ingen forhåndsopprettet app trengs.
  3. Fullfør OAuth-innloggingen i nettleseren. **Dette må en person gjøre.**
  4. Sett `tool_allowlist` til kun lesekall først (`execute_query`), ikke `execute_mutation`.
- **Ferdig når:** `GET /api/v1/mcp/servers` viser serveren som `enabled`, og en chat kan svare på «hva er status på ordre I1/12475?» uten vedlegg.
- **Størrelse:** liten, men avhenger av at noen logger inn.

### A2. Gi modellen et innboks-verktøy *(utvikler)*
- **Nå:** ingen e-postverktøy i det hele tatt. Innboksen finnes i produktet (`/api/v1/inbox/...`, synkroniserer ekte e-post), men modellen kan ikke lese den i en samtale.
- **Gjør:** legg til `inbox_search` og `inbox_get_conversation` i `builtin_tool_defs` i `tool_loop.rs`, med dispatch mot conversation-core på samme mønster som `shipping_get_quotes` (verifisert bearer, org fra forespørselen, aldri fra modellargumenter).
- **Ferdig når:** oppgave 01 kan starte fra en ekte henvendelse i innboksen i stedet for en oppdiktet K1.
- **Størrelse:** middels. Nytt verktøy + tester + rebuild.

### A3. Ekte fraktpriser *(bruker + utvikler)*
- **Nå:** integrasjonen virker, men prisene er mock. `mock-dsv` vant sammenligningen i prøvekjøringen. DHL/UPS/FedEx peker på test-/sandkassemiljøer; DSV og PostNord har ingen ekte adapter.
- **Gjør:**
  1. Skaff produksjonsnøkler for minst én transportør som faktisk brukes (Bring er nærmest — nøkler finnes allerede).
  2. Sett produksjons-URL for den transportøren.
  3. Fjern mock-adapterne fra fleeten når minst én ekte finnes, **eller** merk hvert tilbud med `is_mock`/`environment` slik at svaret kan si «testpris».
  4. La `BRING_LIVE_BOOKING` stå tom til booking faktisk skal skje.
- **Ferdig når:** et fraktestimat i oppgave 01 kommer fra en ekte transportør, og ingen `mock-`-kode er med i svaret.
- **Størrelse:** liten i kode, avhenger av avtaler.

---

## Blokk B — Kunnskapsgrunnlag som mangler

### B1. Ny crawl av aquatiq.com *(utvikler)*
- **Nå:** begge nettsidedokumentene i kunnskapsbasen inneholder bare «Redirecting (308) … The document has moved» (63 og 84 tegn). Kunnskapssøket treffer dem og får ingenting.
- **Gjør:** kjør crawlen på nytt med omdirigeringer fulgt; slett de to tomme dokumentene etterpå (sletteknappen finnes nå i Kunnskap).
- **Ferdig når:** et søk på «Aquatiq hygiene» returnerer faktisk innhold.

### B2. Legg produktdatablad i kunnskapsbasen *(bruker + utvikler)*
- **Nå:** oppgave 03 skrev påstander om bruksområde og daglig bruk som grunnlaget forbød, fordi det ikke finnes noe produktdatablad å bygge på.
- **Gjør:** last opp de virkelige databladene for i det minste produktene i M2 (VirAl Soft Hand, Aqua Des Foam PAA, Aqua Foam Alkachlor, Aqua CIP Alka Active, Aqua Biocip).
- **Ferdig når:** kampanjeutkastene kan si hva produktene brukes til med kilde, i stedet for at vi må forby det.

### B3. Legg kundeservicerutinene i kunnskapsbasen *(bruker)*
- **Nå:** «rutine for ordre på Hold» (K4) er skrevet for prøven. Aquatiqs virkelige rutine finnes ikke i systemet.
- **Gjør:** last opp de gjeldende rutinene for ordrestatus, dellevering og svarfrister.
- **Ferdig når:** oppgave 01 kan hente rutinen med `knowledge_search` i stedet for å få den vedlagt.

### B4. Skaff en ekte kilde til oppgave 04 *(bruker)*
- **Nå:** hele møtenotatet er oppdiktet. Dette er oppgaven som er lengst fra ekte data.
- **Gjør:** velg ett av to — (a) bruk et ekte møtereferat eller prosjektdokument som vedlegg, eller (b) bytt ut oppgave 04 med noe Verevon faktisk har kilder til.
- **Ferdig når:** planen bygger på et dokument noen faktisk skrev.

---

## Blokk C — Visma-uttrekket må bli komplett

### C1. Ordrelinjer for margin og produktgruppe *(utvikler)*
- **Nå:** rapporten kan ikke vise bruttofortjeneste. `unitCost` finnes per linje, men å hente linjer for alle 392 ordrer i to uker krever sidevis paging, og 22 av 179 linjer i stikkprøven manglet `unitCost`.
- **Gjør:**
  1. Page gjennom ordrene for perioden med `orderBy: [{ date: DESC }]` og stopp ved periodestart (`date`-filteret kjører klientside og går i skanntaket).
  2. Aggreger `quantity * unitCost` mot `orderTotal`.
  3. Undersøk hvorfor noen linjer mangler `unitCost` — hvis det er produktoppsett, må det rettes i Visma, ikke i rapporten.
- **Ferdig når:** margin og produktgruppe kan vises uten forbehold.

### C2. Fraktfelter på ordren *(utvikler)*
- **Nå:** `shipping { shipVia shipTerms shipDate shipToAddress }` finnes ikke på `SalesOrderShipping`. Leveringsadresse, vekt og kolli i K3 er satt manuelt.
- **Gjør:** kjør `introspect("salesOrder")` og finn de riktige feltnavnene for leveringsadresse og vekt.
- **Ferdig når:** oppgave 01 henter adresse og vekt fra ordren, slik at fraktforespørselen er helt ekte.

---

## Blokk D — Oppførsel som fortsatt ikke er løst

### D1. Ordgrenser og forbudte påstander i oppgave 03 *(utvikler)*
- Utkastene brøt både ordgrensen (60–90 ord) i tidligere kjøringer og «ingen bruksområdepåstander» i denne. Delvis løst av B2; resten er promptdisiplin.
- **Ferdig når:** tre innlegg på 60–90 ord uten udekkede påstander, uten korreksjonsrunde.

### D2. «Kun vedlegg»-modus *(produktvalg)*
- Kunnskapssøk kjører alltid. I en demo med ekte org-data betyr det at svaret kan trekke inn noe som ikke er en del av oppgaven.
- **Gjør:** avgjør om det skal finnes en modus som begrenser grunnlaget til vedleggene.

---

## Blokk E — Drift og hygiene før opptak

### E1. Rydd minnet igjen etter prøvekjøringene *(utvikler)*
- Minnelageret ble tømt i dag, men nye kjøringer skriver nye minner. Uttrekket skal ikke skrive påstander om personen lenger (rettet), men sjekk listen før opptak.
- **Ferdig når:** `GET /api/v1/memory` ikke inneholder påstander som stammer fra demogrunnlaget.

### E2. Bestem policy for de 22 dupliserte hemmelighetene *(utvikler)*
- Kartleggingen i dag fant 22 Control-eide nøkler med avvikende kopier i andre planer. Bare to var levende feil (rettet). Resten er foreldreløse kopier.
- **Gjør:** enten fjern kopiene, eller dokumenter hvilken fil som eier hver nøkkel.

### E3. Opptak *(bruker)*
- Verktøyet her kan ikke ta skjermopptak. Opptaket må gjøres med en opptaker, én oppgave av gangen, i nye samtaler med samme grunnlag.

### E4. Commit *(utvikler)*
- Alt arbeidet fra i dag ligger ukommitert: rettelsene i Model Plane, BFF, SPA, samt pakkene og loggene under `plans/`.

---

## Anbefalt rekkefølge

Denne rekkefølgen gjelder **hybridsporet etter ny kartlegging**, ikke den nåværende publiseringsporten for de fiktive produktfilmene. Bekreft at hvert «Nå»-punkt fortsatt stemmer før arbeid eller markedsføringspåstander baseres på det.

1. **A3** frakt (ellers viser demoen oppdiktede priser til en kunde)
2. **B1 + B2 + B3** kunnskapsgrunnlag (fjerner de fleste mockene i 01 og 03)
3. **A1** Visma MCP (gjør 01 og 02 ekte ende-til-ende)
4. **C2** fraktfelter, **C1** margin (gjør 01 og 02 komplette)
5. **A2** innboksverktøy (gjør 01 ekte fra første ledd)
6. **B4** kilde til oppgave 04
7. **D1**, **E1** rett før opptak
8. **E3** opptak

Etter 1–4 er oppgave 01 og 02 ekte. Etter 5–6 er alle fire ekte.
