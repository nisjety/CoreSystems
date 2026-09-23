# Produktseksjonen — planlagt, ikke publiseringsklar

**Status 23. september 2026:** Alle fire oppgaver har nå private prøvefilmer på ulike bygg, inkludert en lesbar prosjektplanfilm fra et bestått råopptak. Siste bygg bestod fem sammenhengende private prosjektprøver, men alle fire står fortsatt på 0/5 kvalifiserte sammenhengende **publiseringskjøringer** på ett valgt bygg. Ingen medier er godkjent for offentlig visning. Se [gjeldende opptaksstatus](../../../../docs/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) og [prioritert plan](README.md).

Planlagt innhold for fire faktiske demoer. Dette dokumentet klargjør innhold og medieplassering; det endrer ikke dagens nettside.

## Foreslått innramming
**Overskrift:** Fra kunnskap til handling.
**Ingress:** Se hvordan Verevon bruker bedriftens kunnskap til å svare kunder, forstå tall, lage innhold og planlegge arbeidet.
**Valg i seksjonen:** Kundesvar · Salgsanalyse · Kampanje · Prosjektplan

Behold dagens rolige produktinnramming, farger og overganger. Bruk samme videoflate med fire tydelige valg. Første valg er kundesvar. En valgt demo viser ett opptak, en kort beskrivelse og et lesbart stillbilde fra det faktiske resultatet.

| Valg | Tittel | Undertekst | Planlagt medienavn |
| --- | --- | --- | --- |
| Kundesvar | Fra henvendelse til svarutkast. | Ordrestatus og rutiner blir til et svar med riktig grunnlag. | 01-kundesvar.mp4 |
| Salgsanalyse | Fra tall til prioriteringer. | Se endringene, forstå grunnlaget og finn neste oppfølging. | 02-salgsrapport.mp4 |
| Kampanje | Fra brief til innhold. | Produktkunnskap og bedriftens stemme blir til utkast for flere kanaler. | 03-kampanje.mp4 |
| Prosjektplan | Fra møte til fremdrift. | Notater blir til en plan med ansvar, frister og avhengigheter. | 04-prosjektplan.mp4 |

Dette er tekstutkast for planlagte demoer. Juster til det faktiske, godkjente resultatet før publisering.

## Mediepakke per oppgave
- Råopptak: `<id>-raw-01.<opptakerformat>`, bevart uten kutt.
- Nettklipp: `<id>.mp4`, dempet lyd som standard.
- Stillbilde: `<id>-poster.webp`, fra det ekte resultatet.
- Undertekst: `<id>.nb.vtt`, fra den endelige klippen.
- Registrer råfil, redigert fil, samtalereferanse og varighet i RUN-LOG.md.
- Planlagt plassering for godkjente webmedier: `public/verevon-product-recordings/`, som samsvarer med mediekontrakten.
- Ingen mediefiler eller tomme plassholdere opprettes før opptak finnes. Manifestets mediefelter står på null.

## Oppbygning av én film
1. **Gi oppgaven:** Lesbar instruksjon i det ekte produktet.
2. **Se grunnlaget:** Vis kilden som forklarer det viktigste funnet.
3. **Følg arbeidet:** Vis faktisk prosess og resultat uten å skrive inn ferdige AI-svar.
4. **Form resultatet:** Én kort oppfølging demonstrerer samarbeid.
5. **Bruk arbeidet:** Avslutt på det siste utførte steget, med tydelig status.

## Visning på nettsiden
- Eksplisitt spill/pause og «Se hele oppgaven», ikke kun hover.
- Bytte av oppgave stopper det forrige klippet.
- Kort introduksjon kan bruke et redigert utsnitt; hele opptaket må ha styrbar avspilling.
- Mobil: horisontal velger og samme medieflate, tekst under.
- Redusert bevegelse: statisk poster frem til bruker starter avspilling.
- Last metadata/poster først; hent video når den velges.
- Merking: «Opptak med demodata». Oppgi tidskutt eller fremskynding der det brukes.

## Før klippene kobles inn
En demo er først publiseringsklar når hele scenarioet har bestått fem ganger sammenhengende på samme bygg, korrekthet/ytelse/gjenoppretting/varig lagring er kontrollert, og råopptak, klipp, tekst, poster og faktisk avspilling på mobil og desktop er godkjent. En privat prøvefilm eller bestått komponenttest oppfyller ikke dette alene.

Eksisterende `product-showcase.mp4` er en animert presentasjon av en usendt eksempeloppgave. Den skal ikke merkes som et opptak av disse oppgavene. Erstatt med dokumentert kjøring når mediene er klare.
