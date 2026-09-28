# Opptak og publiseringskontroll

Status 23. september 2026: **fire private prøvefilmer finnes; publiseringsgodkjenning mangler**. Prosjektfilmen er en privat prøve fra det siste byggets femte sammenhengende beståtte prosjektløp, men den kvalifiserte publiseringsserien for alle fire oppgaver mangler. Se [gjeldende opptaksrapport](evidence/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) og [opptaksstatusen](manifest.json). Bruk bare `gpt-5.6-terra` gjennom `openai-codex-subscription`. Testene avviser andre ruter; en manglende forbindelse må løses før opptak.

## Privat prøveopptak

Kjør fra Verevon v3-roten med Node 24, installerte avhengigheter, den lokale Docker-stakken og en gyldig lokal Playwright-innlogging. Innloggingsstate skal aldri kopieres til medier eller publiseres. Opptakskommandoen gjenbruker innloggingen og filmer bare den autentiserte oppgaven.

```powershell
node --experimental-strip-types scripts/capture-product.mjs --mode rehearsal --scenario 01-kundesvar --output 'C:/private/verevon-recordings/customer-take-01'
```

- Velg en **ny absolutt mappe utenfor repository** for hvert forsøk. Kommandoen avviser eksisterende opptak/rapporter og kontrollerer også symbolske lenker. Tidligere feilforsøk beholdes.
- `rehearsal` tillater privat feilsøking mens publiseringsportene er åpne. Den gir aldri publiseringsgodkjenning. Automatisk omkjøring er deaktivert.
- Verktøyet registrerer faktiske Docker-image-ID-er, modell/rute, start/slutt og testresultat i en separat `.run.json`, samt Playwright-rapport i `.json`. Disse er private bevis, ikke mediefiler.
- Kundetesten bruker eksakt prompt, kildepakke og REGI-oppfølging. Den kontrollerer kildehashene, faktiske svar, ordgrenser, dokumentversjon, kildebevaring, kvittering og gjenlasting fra server. Opptaket har lesepauser og et større resultatpanel; pausene legges ikke til målt svartid.
- `02-salgsrapport`, `03-kampanje` og `04-prosjektplan` bruker de eksisterende akseptansetestene med kildevisning, større resultatpanel, lesepauser, REGI-oppfølging og gjenlasting. De registrerer også kildehasher, opptaksmarkører og akseptansestatus. Se [pass 6](evidence/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) for gjeldende utfall. Et råopptak fra en feilet test blir aldri en godkjent film.

`video.webm` lagres under testens mappe når nettleserkonteksten lukkes. Hele oppgaven, faktisk ventetid og eventuelle feil beholdes. Opptakstestene lagrer også `capture.json` med opptaksmarkører og akseptansestatus. Se råfilen før redigering; markørene er veggklokketider som må sammenholdes med videobildene.

## Rediger et bestått prøveopptak

Den vedlikeholdte eksportøren støtter alle fire oppgaver og krever et bestått opptak, en faktisk Terra/abonnementsrute og en JSON-klippeplan bundet til råfilens SHA-256. FFmpeg og ffprobe må være tilgjengelige.

```powershell
node --experimental-strip-types scripts/prepare-product-recording.mjs --capture 'C:/private/verevon-recordings/customer-take-01' --plan 'C:/private/verevon-recordings/customer-edit.json' --output 'C:/private/verevon-recordings/customer-preview-01'
```

Klippplanen inneholder `rawSha256`, `posterTime`, kronologiske `clips` med `start`, `end`, `caption`, og separate `rawCaptions` med `start`, `end`, `text`. Tider er sekunder i den faktiske råfilen. Ingen overlapp, ugyldig tekst eller klipp utenfor råfilen tillates. Hovedfilmen skal være 45–60 sekunder; støttefilmene skal være 25–40 sekunder.

Eksporten leverer:

- `short.mp4`: H.264, kronologiske tidskutt og fast merking som privat prøveopptak med fiktive data. Ingen oppdiktede bilder, endrede svar eller påstand om svartid.
- `raw.webm`: en bitidentisk kopi av hele råopptaket.
- `poster.png`: et faktisk valgt resultatbilde.
- `captions.vtt` og `raw-captions.vtt`: separate norske beskrivelser av handlingene, med tidskoder til hver film. Det er ingen innspilt tale.
- `media-review.json`: kildeformat, faktisk varighet/bildefrekvens, klippeplan og filhash. Ingen publiseringsgodkjenning.

Eksportøren dekoder begge filmer, sjekker varighet og verifiserer at råfilen er uendret. Tidligere eksportmapper overskrives ikke. Kontroller undertekster, faktiske klippgrenser, lesbarhet og sluttflate visuelt. Et bestått automatisk mediesjekk erstatter ikke denne kontrollen.

Verifisert kundeprøve: 1440 × 900, VP8, **25 fps**; kortfilmen har et ekstra 60-pikslers felt med merking. Dersom ferdig leveranse krever ekte 30 fps, må et nytt kildeopptak tas med egnet opptaker.

## Kvalifisert opptak og publisering

1. Stabiliser prosjektforløpet: flere komplette private førstetur/REGI/gjenåpningsløp er nå bevist, men andre løp har tidsavbrudd eller falsk nettleseravvisning. Førsteutkast, separat status og eventuelle rettelser må holde seg til kildene; korrigert innhold må få uavhengig fullstendig kontroll innen eksisterende 90 sekunders artefaktfrist. Ikke tell en tidsavbrutt eller automatisk omkjørt samtale som bestått.
2. Velg ett registrert bygg og kjør deretter hver oppgave med eksakt REGI-oppfølging **fem ganger sammenhengende** på samme `gpt-5.6-terra`-abonnementsrute og uendrede kildefiler. Behold alle feilforsøk og kontroller både chat og artefakt mot fasiten. En feil nullstiller serien for den oppgaven; eldre private filmer på andre bygg teller ikke.
3. Fullfør correctness, repeatability, durability, recovery, trust, experience og performance med konkrete bevis. Mål p50/p95-svartid mot en på forhånd fastsatt produktgrense, test gjenoppretting etter feil/omstart, bekreft artefaktversjoner etter ny nettleser, og kontroller mobil, desktop, tastatur, tekstspor og redusert bevegelse. Oppdater manifestets bevislenke og faktiske heltall for `currentEvidence.consecutivePassesOnReleaseBuild`. Sett `ready-for-recording` først når alle porter er `passed`.
4. Bruk `--mode release` med den samme opptakskommandoen og en ny privat mappe. Den avviser opptak når portene eller femkjøringskravet ikke er oppfylt. Den gamle `PRODUCT_RECORDING_CAPTURE=1` betyr fortsatt kvalifisert opptak og omgår ingen porter.
5. Fullfør visuell/faglig gjennomgang av alle fire filmer, råfiler, postere og separate norske tekstspor. Bruk 45–60 sekunder for hovedfilmen og 25–40 for støttefilmene. Beskriv tidskutt tydelig; klipplengde er ikke kjøretid.
6. Legg bare godkjente medier under `public/verevon-product-recordings/`. Fyll manifestets `media.raw`, `video`, `poster`, `captions` og `rawCaptions`. Hver oppgave trenger `status: media-approved`, datert `approval` med bevis, bygg og minst fem sammenhengende beståtte kjøringer, samt presis `editingNote`. Alle fire må være klare før global status settes til `approved`.
7. Verifiser den virkelige hjemmesiden på desktop/mobil: fire oppgavevalg, én film om gangen, ingen automatisk avspilling, pause ved bytte, norske tekstspor, tastatur, redusert bevegelse og håndtering av mediefeil. Bruk fullskjerm når et dokument ikke er lesbart i et lite videovindu.

Serverkontrollen avviser manglende/tomme filer, feil filtype, private/eksterne stier og ugyldig repetisjonsbevis. Den offentlige komponenten får bare offentlige medier og etiketter. Private prøveopptak, innloggingsstate, logger og spor skal aldri legges i `public`.
