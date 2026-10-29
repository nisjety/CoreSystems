# Fire oppgaver til produktseksjonen

> **Historisk øyeblikksbilde fra 14. september.** Bruk [den vedlikeholdte pakken](../../product-recordings/README.md) og dens manifest for nye kjøringer. Statusene og mediefeltene i denne kopien er ikke gjeldende godkjenning; tidligere filer og promptgrunnlag er bevart.

Klargjort 14. september 2026. Alle forretningsdata og personer i denne pakken er fiktive.
Demobedrift: **Fjordform**, en leverandør av møbler og belysning til arbeidsplasser.

**Status: Oppgaver og kilder er klare. Ingen av oppgavene er kjørt eller tatt opp ennå.**
Den lokale appen åpnet på innloggingssiden under klargjøringen. Kildekoden har flater for vedlegg, kilder, arbeid og resultater; faktisk kjøring må bekreftes etter innlogging.

## De fire oppgavene

| # | Oppgave | Resultat vi vil filme | Viser |
| --- | --- | --- | --- |
| 01 | [Svar kunden med riktig grunnlag](01-kundesvar/prompt.txt) | Et presist svarutkast med kildehenvisninger | Sammenholde en henvendelse, ordrestatus og rutiner |
| 02 | [Gjør salgstallene forståelige](02-salgsrapport/prompt.txt) | En ukesrapport med beregninger og prioriterte tiltak | Analysere data og skille fakta fra forklaringer |
| 03 | [Gjør briefen til kampanjeinnhold](03-kampanje/prompt.txt) | Tre LinkedIn-utkast og én e-post | Lage konsistent innhold ut fra bedriftens kunnskap |
| 04 | [Gjør møtet til en prosjektplan](04-prosjektplan/prompt.txt) | Oppgaver med eiere, frister, avhengigheter og risiko | Gjøre ustrukturerte notater om til arbeid teamet kan følge |

Vi tar opp **01 først**. Det blir hovedfilmen. De tre andre viser bredden i produktseksjonen.

## Klar til prøvekjøring

1. Logg inn i [Verevon lokalt](http://localhost:5173/) og velg en egnet demoarbeidsflate.
2. Opprett en ny samtale for hver oppgave. Bruk tittel fra tabellen.
3. Legg ved kun filene listet som `inputs` i [manifestet](manifest.json). Kontrollfasiten og reginotatene skal ikke være AI-grunnlag.
4. Lim inn oppgavens `prompt.txt`. Kjør den først uten opptak for å kontrollere at vedlegg, kjøring og resultater virker.
5. Sammenlign det faktiske resultatet med [kontrollfasiten](CONTROL-NOTES.md). Korriger kilder eller oppgave ved behov; ikke skriv et ferdig svar inn i grensesnittet for å få en vellykket film.
6. Start en ny samtale med samme grunnlag for det faktiske opptaket. Kjør én oppgave av gangen.

Hver oppgavemappe har også en `samtalestart.txt` med prompt og alle kilder samlet, klar til innliming. Bruk den hvis vedlegg ikke kan leses. Registrer dette i kjøreloggen. Det viser arbeid med gitt kontekst, ikke søk gjennom integrerte bedriftssystemer.

## Opptak

- Norsk grensesnitt, rolig museføring og samme utsnitt i alle fire filmer. Mål: 1440 × 900 eller 1920 × 1080, 30 bilder per sekund.
- Start opptaket før oppgaven sendes. Behold råopptaket fra instruksjon til resultat.
- Vis oppgave → relevant kilde → faktisk arbeid → lesbart resultat → en reell oppfølging.
- Bruk oppfølgingsprompten i reginotatet etter at første resultat er kontrollert. Det viser at arbeidet kan formes videre.
- Redigerte klipp: ca. 45–60 sekunder for hovedfilmen og 25–40 sekunder for de øvrige. Dette er klippelengder, ikke løfter om kjøretid.
- Marker vesentlige tidskutt/fremskynding. Legg på undertekst. Behold en rolig sluttflate i 3 sekunder.
- Film lagring, opprettelse eller godkjenning bare dersom appen faktisk utfører handlingen. Et dokument med en oppgavetabell er ikke det samme som opprettede oppgaver.
- Ingen sending, publisering, planlagte utsendelser eller endringer i ekte kunde-/økonomisystemer er nødvendig for disse oppgavene.

Opptak er ikke startet i denne klargjøringen. Nettleserverktøyet her tilbyr skjermbilder, men ingen skjermopptaksfunksjon. Bruk en tilgjengelig opptaker ved gjennomføringen.

## Filer og neste steg

- [Produktseksjon og klipp](PRODUCT-SECTION.md): overskrifter, oppbygning og ferdige plasseringer.
- [Kjørelogg](RUN-LOG.md): fylles ut med faktisk samtale, resultat og opptak.
- [Kontrollfasit](CONTROL-NOTES.md): for regissør/tester, ikke for Verevon.
- [Manifest](manifest.json): fire opptaksplasser, kildefiler og status. Mediefeltene er tomme til opptakene finnes.

Oppgavene er uavhengige og kan kjøres i valgfri rekkefølge. Alle bruker en fast demodato: **14. september 2026**.
