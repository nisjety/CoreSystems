# Fire oppgaver til produktseksjonen

Klargjort 14. september 2026. Alle forretningsdata og personer i denne pakken er fiktive.
Demobedrift: **Fjordform**, en leverandør av møbler og belysning til arbeidsplasser.

**Status 23. september 2026: Fire private prøvefilmer finnes; publisering er fortsatt sperret.** Kundesvaret har en prøvefilm på 45,64 sekunder og et ubrutt opptak på 121,08 sekunder. Salgsanalysen har en prøvefilm på 35,04 sekunder og et ubrutt opptak på 224,48 sekunder. Kampanjen har en prøvefilm på 30,32 sekunder og et råopptak på 199,36 sekunder. Prosjektplanen har nå en lesbar privat prøvefilm på 34,52 sekunder fra et bestått 137,88-sekunders råopptak på det siste bygg, med separat intern status, gjenåpning og norske tekstspor. Alle bruker bare GPT 5.6 Terra via ChatGPT-abonnement; de tre eldre filmene er fra andre testbygg. Ingen er publiseringsgodkjent.

Prosjektplanen har nå **fem sammenhengende komplette private prøver på siste bygg**, med kildekontrollert førsteutkast, REGI-oppfølging og gjenåpning. Planen og den interne statusnoten lagres som **to separate artefakter**. Korte firekolonners planer er lesbare i opptaksvisningen. Tidligere bygg fant tidsavbrudd i kildekontroll og feilaktige avvisninger i nettleserfasiten; feilforsøkene er bevart. Se [gjeldende rapport i pass 6](../../../../docs/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) og [privat filmvisning](http://127.0.0.1:5292/) for de tre eldre prøvefilmene. Fem sammenhengende **kvalifiserte publiseringskjøringer** per oppgave på ett valgt bygg, målt ytelse og gjenoppretting gjenstår. Manifestet bevarer historikken fra 14. september adskilt fra gjeldende godkjenning.

## Prioritert arbeid før publisering

1. **Kvalifiser prosjektplanen sammen med de tre andre oppgavene.** Fem komplette private prosjektforløp på ett bygg er bevist, men dette er ikke publiseringsserien. Førsteutkast, separat status, reparasjon og uavhengig fullstendig recheck må fortsette å bestå innen eksisterende 90-sekunders artefaktfrist uten skjulte omkjøringer. Særlig må ukjent godkjenningsstatus, vedtatte kontra oppgitte datoer og betinget teknisk feilretting forbli kildebundet.
2. **Vurder den private prosjektfilmen.** Kontroller v57-råopptak, klipp, tekstspor, resultatets lesbarhet og sannferdig status på det siste bygg. Et bestått prøveklipp fra samme bygg er fortsatt ikke publiseringsgodkjent.
3. **Velg ett publiseringsbygg.** Kjør alle fire oppgaver fem ganger sammenhengende uten omkjøring, med samme modellrute og uendrede kildefiler. Et bestått opptak fra et annet bygg teller ikke mot disse fem.
4. **Lukk de øvrige portene.** Mål faktisk p50/p95-svartid mot en fastsatt produktgrense, og prøv feil/gjenoppretting, lagring etter omstart og ny nettleser, mobil/desktop, tastatur, undertekster og redusert bevegelse. Kontroller at tekst, status og film bare lover det appen faktisk gjorde.
5. **Godkjenn medier og nettsiden.** Gjennomgå alle fire råfiler og klipp faglig og visuelt. Først når manifestets porter er bestått, legg godkjente medier i den offentlige mediemappen og test den virkelige produktseksjonen. Se [opptaks- og publiseringskontrollen](RECORDING-WORKFLOW.md).

## De fire oppgavene

| # | Oppgave | Resultat vi vil filme | Viser |
| --- | --- | --- | --- |
| 01 | [Svar kunden med riktig grunnlag](01-kundesvar/prompt.txt) | Et presist svarutkast med kildehenvisninger | Sammenholde en henvendelse, ordrestatus og rutiner |
| 02 | [Gjør salgstallene forståelige](02-salgsrapport/prompt.txt) | En ukesrapport med beregninger og prioriterte tiltak | Analysere data og skille fakta fra forklaringer |
| 03 | [Gjør briefen til kampanjeinnhold](03-kampanje/prompt.txt) | Tre LinkedIn-utkast og én e-post | Lage konsistent innhold ut fra bedriftens kunnskap |
| 04 | [Gjør møtet til en prosjektplan](04-prosjektplan/prompt.txt) | Oppgaver med eiere, frister, avhengigheter og risiko | Gjøre ustrukturerte notater om til arbeid teamet kan følge |

Vi tar opp **01 først**. Det blir hovedfilmen. De tre andre viser bredden i produktseksjonen.

## Slik kjøres en privat prøve

1. Logg inn i [Verevon lokalt](http://localhost:5173/) og velg en egnet demoarbeidsflate.
2. Opprett en ny samtale for hver oppgave. Bruk tittel fra tabellen.
3. Legg ved kun filene listet som `inputs` i [manifestet](manifest.json). Kontrollfasiten og reginotatene skal ikke være AI-grunnlag.
4. Lim inn oppgavens `prompt.txt`. Kjør den først uten opptak for å kontrollere at vedlegg, kjøring og resultater virker.
5. Sammenlign det faktiske resultatet med [kontrollfasiten](CONTROL-NOTES.md). Korriger kilder eller oppgave ved behov; ikke skriv et ferdig svar inn i grensesnittet for å få en vellykket film.
6. Start en ny samtale med samme grunnlag for det faktiske opptaket. Kjør én oppgave av gangen.

Hver oppgavemappe har også en `samtalestart.txt` med prompt og alle kilder samlet, klar til innliming. Bruk den hvis vedlegg ikke kan leses. Registrer dette i kjøreloggen. Det viser arbeid med gitt kontekst, ikke søk gjennom integrerte bedriftssystemer.

## Opptak

- Norsk grensesnitt, rolig museføring og samme utsnitt i alle fire filmer. Det verifiserte nettleseropptaket er 1440 × 900 ved 25 bilder per sekund. Den redigerte prøvefilmen har et ekstra felt med tydelig merking. Ekte 30 fps krever en annen, verifisert opptaker; konvertering gir ikke flere kildebilder.
- Start opptaket før oppgaven sendes. Behold råopptaket fra instruksjon til resultat.
- Vis oppgave → relevant kilde → faktisk arbeid → lesbart resultat → en reell oppfølging.
- Bruk oppfølgingsprompten i reginotatet etter at første resultat er kontrollert. Det viser at arbeidet kan formes videre.
- Redigerte klipp: ca. 45–60 sekunder for hovedfilmen og 25–40 sekunder for de øvrige. Dette er klippelengder, ikke løfter om kjøretid.
- Marker vesentlige tidskutt/fremskynding. Legg på undertekst. Behold en rolig sluttflate i 3 sekunder.
- Film lagring, opprettelse eller godkjenning bare dersom appen faktisk utfører handlingen. Et dokument med en oppgavetabell er ikke det samme som opprettede oppgaver.
- Ingen sending, publisering, planlagte utsendelser eller endringer i ekte kunde-/økonomisystemer er nødvendig for disse oppgavene.

Playwright bevarer ubrutte nettleseropptak fra autentiserte akseptansetester. Se [opptaksprosedyren](RECORDING-WORKFLOW.md) for egne moduser for privat prøveopptak og kvalifisert opptak. Et testopptak er ikke en godkjent markedsføringsfilm. Manifestets offentlige mediefelt forblir tomme til alle fire filmer og publiseringskrav er godkjent.

## Filer og neste steg

- [Produktseksjon og klipp](PRODUCT-SECTION.md): forslag til overskrifter, oppbygning og medieplassering etter godkjenning.
- [Kjørelogg](RUN-LOG.md): historiske kjøringer og gjeldende kontrollpunkt; nye forsøk føres med faktisk samtale, resultat og opptak.
- [Kontrollfasit](CONTROL-NOTES.md): for regissør/tester, ikke for Verevon.
- [Manifest](manifest.json): fire opptaksplasser, kildefiler og status. Mediefeltene er tomme til opptakene finnes.

Oppgavene er uavhengige og kan kjøres i valgfri rekkefølge. Alle bruker en fast demodato: **14. september 2026**.
