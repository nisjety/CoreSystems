# Kjøre- og opptakslogg

> **Gjeldende status — 23. september 2026:** Den siste [pass-6-rapporten](evidence/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) og [loggposten nederst](#23092026--pass-6-separat-prosjektstatus-og-privat-prøvefilm) styrer publiseringsstatus. Fire private prøvefilmer finnes; prosjektfilmen er en lesbar prøve fra det siste byggets femte sammenhengende beståtte prosjektløp. Tidligere feilforsøk er bevart som historikk. Alle fire oppgaver står på 0/5 kvalifiserte sammenhengende kjøringer, og ingen offentlige medier er godkjent. Opptak er teknisk mulig med Playwright; de tidligere verktøybegrensningene er historiske. Se [opptaksprosedyren](RECORDING-WORKFLOW.md).

Status ved klargjøring, 14. september 2026:
- [x] Fire oppgavebeskrivelser
- [x] Fiktive kildepakker og CSV
- [x] Kontrollgrunnlag og reginotater
- [x] Produkttekster og planlagte medieplasser
- [x] Innlogget demoarbeidsflate (AQUATIQ AS — se merknad om arbeidsflate under 01)
- [x] Prøvekjøring med lesbare kilder og faktisk resultat (alle fire godkjent 14.09 på ferdig bygg, med ekte vedlegg — se «Full prøvekjøring» nederst)
- [ ] Råopptak (kan ikke gjøres fra dette verktøyet — bruk opptaker og samme `samtalestart.txt`-bundle i ny samtale per oppgave)
- [ ] Resultatkontroll, redigering, poster og undertekst
- [ ] Koblet inn og kontrollert i produktseksjonen

| Oppgave | Prøvekjøring | Kontroll | Samtale/kjøring | Råopptak | Nettklipp |
| --- | --- | --- | --- | --- | --- |
| 01 Kundesvar | Kjørt 14.09 (3 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G70YVQVYHGHN6AER01VVRZ` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 02 Salgsrapport | Kjørt 14.09 (3 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G75EFP4NZDS2S0J4P40FC3` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 03 Kampanje | Kjørt 14.09 (2 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G7BB4X7CK7PGSYVVE0DZNA` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 04 Prosjektplan | Kjørt 14.09 (2 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G7G9Q2AVB3JAW7RW5BV715` | Ikke mulig fra verktøyet (kun stillbilder) | — |

## Mal — fylles ut for hver kjøring
- Oppgave og forsøksnummer:
- Dato og demoarbeidsflate:
- Faktisk samtalereferanse / kjørings-ID:
- Kilder brukt (vedlegg / innlimt kontekst / indekserte dokumenter):
- Modell og tilgjengelige verktøy:
- Resultat som faktisk ble laget:
- Hvor resultatet finnes / eventuell lagringskvittering:
- Avvik fra kontrollgrunnlaget:
- Oppfølging og endring:
- Opptaksfil, råvarighet og redigert varighet:
- Tidskutt/fremskynding:
- Poster og undertekst:
- Kontrollør og godkjent dato:

## 01 Kundesvar — forsøk 1 (prøvekjøring, avvik)
- Oppgave og forsøksnummer: 01 Kundesvar, forsøk 1.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (den innloggede brukerens ekte org). Det finnes ingen egen Fjordform-arbeidsflate; se avvik.
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FFZTKVPEV8WZWXDFX0Q43J`.
- Kilder brukt: `kildepakke.md` lagt ved som ekte vedlegg via komponistens filvelger (2 KB, text/markdown). I tillegg trakk modellen selv inn org-kunnskap (Kilder-fanen viste `chat-upload`, `sharepoint`, `quarry`, `https://www.aquatiq.com/`) og «Brukte 5 minner fra tidligere samtaler».
- Modell og tilgjengelige verktøy: Claude Sonnet. 7 345 tokens inn / 110 ut, 155,7 s, 88 % sikkerhet, $0,0237. 14 steg, verktøy inkl. kunnskapssøk (alltid påslått i chat; ingen «kun vedlegg»-modus finnes i komponisten — bare «kunnskap» / «kunnskap og nett»).
- Resultat som faktisk ble laget: Ett dokument-artefakt «Svarutkast til Nordvik Studio – ordre FF-1042» i 5 versjoner, pluss to ekstra artefakter med samme tittel. Chat-svaret sa: «Kunnskapsbasen inneholder kun begynnelsen av K2 (avkortet ved ‘bestilt 8…’). Fullstendig lager- og fraktstatus mangler.»
- Hvor resultatet finnes: kun i den levende samtalen/lokalt øyeblikksbilde. Den varige tråden (`/api/v1/chat/threads/<id>/messages`) har 0 artefakter på assistentturen — artefaktene overlevde ikke til lesestien.
- Avvik fra kontrollgrunnlaget: Modellen fikk kildepakken **avkortet** til K1 + første linje av K2. Årsak: vedlegg går via Data Plane-innlesing som deler opp asynkront, så turen som bærer filen kan ikke bruke hele innholdet (kjent forbehold i VEREVON_CHAT_DESIGN.md, punkt 7). Forhåndsvisningen i nettleseren viste hele filen — avkortingen skjedde på innlesingsstien, ikke i klienten. Kontrollpunktene om 9+3, 17.09 som lagerankomst m.m. kunne derfor ikke vurderes på et fullt grunnlag. Kryssforurensning: ekte Aquatiq-kilder og minner ble hentet inn i et fiktivt Fjordform-scenario, fordi prompten sier «rutinene våre» og arbeidsflaten er Aquatiq.
- Oppfølging og endring: Ikke sendt. Forsøk 2 kjøres med innlimt `samtalestart.txt` (READMEs egen reserveløsning når vedlegg ikke kan leses).
- Opptaksfil, råvarighet og redigert varighet: Ingen — nettleserverktøyet har ikke skjermopptak. Stillbilder tatt ved oppgave, kilde, arbeid og resultat.
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Ikke godkjent (avvik). Kontrollert 14.09.2026.

Produktfunn fra forsøk 1 (til utvikling, ikke til opptak):
1. Et tekstvedlegg i samme tur nås bare delvis av modellen (asynkron oppdeling). Demoen forutsetter «legg ved og få svar» — det holder ikke i dag.
2. Agentløkka reviderte samme artefakt fem ganger og opprettet to til med samme tittel i én kjøring (14 steg, 155 s) i stedet for å konvergere.
3. Artefakter fra kjøringen finnes ikke i den varige trådlesingen (0 artefakter via API), bare i den levende strømmen.
4. Ingen mulighet til å avgrense grunnlaget til vedlegg alene; kunnskapssøk er alltid med, og «Midlertidig samtale» er utilgjengelig fordi ingen katalogmodell er attestert for null datalagring.

## 01 Kundesvar — forsøk 2 (prøvekjøring, godkjent)
- Oppgave og forsøksnummer: 01 Kundesvar, forsøk 2, med oppfølging fra REGI.md.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (ingen Fjordform-arbeidsflate finnes).
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FG9FE8F9K82VVEZR14571Z`.
- Kilder brukt: innlimt `samtalestart.txt` ordrett (prompt + hele kildepakken i meldingen), jf. READMEs reserveløsning når vedlegg ikke kan leses. Ingen filer lagt ved. Kilder-fanen ble ikke vist — ingen eksterne kunnskapstreff. «Brukte 5 minner fra tidligere samtaler» sto på turen, men ingenting fra Aquatiq havnet i teksten.
- Modell og tilgjengelige verktøy: Claude Sonnet. Første tur: 5 011 tokens inn / 118 ut, 31,2 s, 82 % sikkerhet, $0,0168, 1 steg, 1 artefakt. Oppfølging: 10 steg (1 feilet), 84 % sikkerhet, 3 artefakter der ett gikk gjennom 6 versjoner.
- Resultat som faktisk ble laget: Dokument-artefakt «Svarutkast – ordre FF-1042 (Nordvik Studio)»: kundeutkast på ca. 100 ord («Hei Nora, og takk for at du tok kontakt! … Vennlig hilsen [Navn], Fjordform») etterfulgt av «⚠ Intern merknad – kun for internt bruk» med kildetabell K1–K4. Etter oppfølgingen: «Svarutkast til Nora – ordre FF-1042», ca. 95 ord i kundeteksten, varmere tone, usikkerhet om leveringsdato beholdt, kildeliste beholdt; modellen la til en 🎉 i hilsenen.
- Hvor resultatet finnes: i samtalens Resultat-fane (sidepanelet). Ikke lagret noe annet sted; ingen melding sendt, ordren ikke endret.
- Avvik fra kontrollgrunnlaget: Ingen. Kontrollert punkt for punkt: 12 = 9 pakket + 3 på påfyll ✔; 17.09 omtalt som ankomst til vårt lager, ikke levering ✔; «transportøren har ennå ikke hentet pakkene», ingen «sendt» ✔; ingen leveringsgaranti før 21.09 ✔ («Vi setter alt inn på…» lover ikke); dellevering undersøkes, tidspunkt og tillegg avklares av logistikk før booking ✔; K3 brukt, K4 eksplisitt «ikke brukt» ✔; neste status senest 15.09 ✔; utkast og intern merknad adskilt ✔. Ordgrense: 150 gjelder svaret; kundeteksten er ca. 100 ord, kildetabellen ligger utenfor.
- Oppfølging og endring: «Gjør svaret litt varmere og kortere, maks 100 ord. Behold usikkerheten rundt leveringsdatoen og den interne kildeoversikten.» sendt i samme samtale. Resultat: nytt utkast som oppfyller alle tre krav. Merk: oppfølgingen tok 10 steg og skapte tre artefakter (én i 6 versjoner) for en 100-ords omskriving; ett steg feilet («Siste steg feilet» i Arbeid). execution-core logget samtidig seks h2 `GoAway`-tilkoblingsfeil — sannsynlig årsak til det feilede steget, ikke modellen.
- Opptaksfil, råvarighet og redigert varighet: Ingen opptak — verktøyet har ikke skjermopptak. Stillbilder tatt ved: oppgave sendt, arbeid underveis (Kilder/Arbeid), resultat i sidepanelet, oppfølging med revidert utkast. Selve opptaket må gjøres med en opptaker; bruk nøyaktig samme bundle i en ny samtale (README steg 6).
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Godkjent mot CONTROL-NOTES.md 01, 14.09.2026 (prøvekjøring; opptak gjenstår).

Produktfunn fra forsøk 2 (til utvikling):
5. Oppfølgingen konvergerte ikke. Arbeid-fanen viser verktøykallene: «Create artifact» 8 ganger, «Update artifact» 1 gang og «Reattach context» 2 ganger for en 100-ords omskriving — resultat 3 artefakter, ett i 6 versjoner. Samme mønster som forsøk 1 (5 versjoner + 2 ekstra artefakter). Løkka bør oppdatere ett artefakt, ikke opprette nye med samme tittel.
6. «Siste steg feilet» vises uten navn på steget eller årsak i Arbeid-fanen; årsaken måtte hentes fra execution-core-loggen.

## 02 Salgsrapport — forsøk 1 (prøvekjøring, avvik)
- Oppgave og forsøksnummer: 02 Salgsrapport, forsøk 1.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (ingen Fjordform-arbeidsflate finnes).
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FGNVW1ZCYKECN8RRD4B54R`.
- Kilder brukt: innlimt `samtalestart.txt` ordrett (prompt + kildepakke S1–S3 + salg.csv som tekst), jf. READMEs reserveløsning etter avkortingsfunnet i 01. Ingen filer lagt ved. «Brukte 5 minner fra tidligere samtaler» sto på turen; ingen Aquatiq-innhold i rapporten.
- Modell og tilgjengelige verktøy: Claude Sonnet. 5 260 tokens inn / 231 ut, 165,4 s, 76 % sikkerhet, $0,0192. 5 steg, 3 feilet: `Code interpreter` ble kalt tre ganger (10:31–10:32) og avvist hver gang med «capability policy unavailable». Modellen regnet deretter for hånd og opprettet to dokument-artefakter med samme tittel («Fjordform – Ukentlig salgsrapport, uke 37 vs. uke 36», 4 734 og 5 187 tegn).
- Resultat som faktisk ble laget: Rapport i sidepanelet med sammendrag, beregningsgrunnlag, sammenligningstabeller (totalt og per gruppe), fakta vs. mulige forklaringer, tre oppfølginger med foreslått eier og frist, begrensninger og kildelinje.
- Hvor resultatet finnes: samtalens Resultat-fane (sidepanelet). Ingenting distribuert.
- Avvik fra kontrollgrunnlaget: **Tabellene er riktige, sammendraget er feil.** Seksjon 3 stemmer med CONTROL-NOTES 02 punkt for punkt: omsetning 360 000 → 360 000 (0 %), varekost 237 000 → 235 320, bruttofortjeneste 123 000 → 124 680 (+1 680, +1,4 %), vektet margin 34,2 % → 34,6 % (+0,5 pp); per gruppe Belysning 120 000/150 000 (+25 %, 40 %/40 %), Skrivebord 180 000/126 000 (−30 %, BF 54 000/35 280, 30 %/28 %), Oppbevaring 60 000/84 000 (+40 %, 35 %/35 %) ✔; 48 000 i utsatte tilbud eksplisitt holdt utenfor fakturert salg ✔; kampanje, tilbud og innkjøpspris omtalt som ubekreftede forklaringer ✔; oppfølginger knyttet til tilbud, skrivebordsmargin og kampanjedata ✔. Men seksjon 1 (sammendrag) sier «Totalomsetningen steg 10,7 %» og «bruttomargin falt 1,3 prosentpoeng» — begge motsier egne tabeller. Første totaltabell i seksjon 2 inneholder «360 000 → 398 000» og «31,3 % → se under», etterfulgt av «(Rettelse: summer nedenfor er korrekte tall)». Setningen om skrivebordsmarginen («netto snittpris per enhet er identisk») er selvmotsigende. Dokumentet er ikke beslutningsklart uten redigering → ikke godkjent.
- Oppfølging og endring: REGI-oppfølgingen ikke sendt på et avvikende resultat. Årsak til de feilede stegene funnet i driftsmiljøet: `model-plane-capability-core-1` (startet 11.09) kjørte med tom `CAPABILITY_CORE_DECISION_SIGNING_KEY` (oppstartslogg: «capability decision evidence signer unavailable; execution allow responses will lack per-call proof»), mens execution-core har den tilhørende offentlige nøkkelen og avviser usignert «allow» som «invalid decision evidence». Nøkkelen finnes i `apps/Model Plane/deploy/.env.generated-secrets`; containeren var opprettet utenfor launcherens miljøkjede. Rettet ved å gjenskape via `build-verevon-services.sh --from model --skip-build`. Forsøk 2 kjøres i ny samtale med samme bundle etterpå.
- Opptaksfil, råvarighet og redigert varighet: Ingen opptak (verktøyet har ikke skjermopptak). Stillbilder: oppgave sendt, Arbeid-fanen med tre avviste verktøykall, resultat i sidepanelet.
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Ikke godkjent (avvik). Kontrollert 14.09.2026.

Produktfunn fra 02 forsøk 1 (til utvikling):
7. Når kodetolkeren er utilgjengelig, faller modellen tilbake til hoderegning og leverer et dokument der sammendraget motsier tabellene, med en «Rettelse» midt i teksten i stedet for å skrive om. Kjøringen bør enten stoppe med tydelig feilmelding når et verktøy avvises tre ganger, eller verifisere sammendrag mot tabell før artefaktet ferdigstilles.
8. «capability policy unavailable» vises til brukeren uten forklaring; årsaken (manglende signeringsnøkkel i capability-core) var bare synlig i containerlogger. Verevon burde skille konfigurasjonsfeil fra policyavslag i Arbeid-fanen.
9. Miljødrift igjen: en container opprettet utenfor `build-verevon-services.sh` mistet en nøkkel fra `.env.generated-secrets` (samme mønster som e-postsynk 401 tidligere i dag). En oppstartssjekk som feiler hardt når signeringsnøkkelen mangler mens execution-core krever bevis, hadde avslørt dette 11.09.

## 02 Salgsrapport — forsøk 2 (prøvekjøring, godkjent)
- Oppgave og forsøksnummer: 02 Salgsrapport, forsøk 2, med oppfølging fra REGI.md.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (ingen Fjordform-arbeidsflate finnes). Kjørt etter at capability-core var gjenskapt med signeringsnøkkel (se forsøk 1).
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FHMGZX0ED4GT8F4F7FGMYF` (første kjøring `01M2FHMH1GV9VKMB17NJDGH2M9`).
- Kilder brukt: innlimt `samtalestart.txt` ordrett (prompt + S1–S3 + salg.csv som tekst). Ingen filer lagt ved. Ingen Kilder-fane/eksterne treff. «Brukte 5 minner fra tidligere samtaler» på alle turer; ingenting fra Aquatiq i teksten.
- Modell og tilgjengelige verktøy: Claude Sonnet. Første tur: 5 248 tokens inn / 384 ut, 69,4 s, 84 % sikkerhet, $0,0215, 2 steg: `Code interpreter` (pandas-aggregering, kjørte nå) + `Create artifact`. Oppfølging 1: 6 874 inn / 71 ut, 38,4 s, 82 %, 4 steg (1 feilet). Oppfølging 2: 3 steg, 68 % sikkerhet.
- Resultat som faktisk ble laget: Dokument «Salgsrapport uke 37 vs. uke 36 — Fjordform» (3 393 tegn): sammendrag, totaltabell, per-gruppe-tabell, beregningsgrunnlag, «Målte fakta» vs. «Mulige årsaker (ikke bekreftet)», tre oppfølginger med eier og frist, forbehold. Deretter «Ledernotat — uke 37 vs. uke 36» (v1–v2) og etter korreksjon «Ledernotat – Salg uke 37 vs. uke 36» (v3, 92 ord).
- Hvor resultatet finnes: samtalens Resultat-fane (sidepanelet), to dokumenter. Ingen budsjetter/priser endret, ingenting distribuert.
- Avvik fra kontrollgrunnlaget: **Rapportdokumentet: ingen.** Totalt 360 000 → 360 000 (0 %), varekost 237 000 → 235 320, BF 123 000 → 124 680 (+1,4 %), vektet margin 34,2 % → 34,6 % (+0,5 pp; kodetolkeren ga 0,341667/0,346333 = 34,17 %/34,63 %) ✔. Per gruppe Belysning 120 000/150 000 +25 %, 48 000/60 000, 40 %/40 % ✔; Skrivebord 180 000/126 000 −30 %, 54 000/35 280, 30 %/28 % (−2,0 pp) ✔; Oppbevaring 60 000/84 000 +40 %, 21 000/29 400, 35 %/35 % ✔. 48 000 holdt utenfor og eksplisitt begrunnet ✔. Kampanje, tilbud og innkjøpspris under «ikke bekreftet» ✔. Oppfølginger knyttet til tilbud, skrivebordsmargin/prisjustering og kampanjedata ✔. Småfeil: chat-sammendraget (ikke dokumentet) skrev «+1 360 kr» og «−0,2 pp» der dokumentet har +1 680 og −2,0 pp; dokumentets forklaringssetning om varekostprosent for Skrivebord oppgir «48,0 % mot 46,7 %» (riktig er 70,0 % → 72,0 %). Tabellene er riktige.
- Oppfølging og endring: REGI-oppfølgingen «Kok dette ned til et ledernotat på maks 120 ord, med de tre viktigste tallene og første anbefalte oppfølging.» ga et notat på 95 ord med riktig struktur, men **feil bruttofortjeneste (124 560 / +1 360)** — modellen regnet på nytt i hodet i stedet for å bruke tabellen. Ett steg feilet: `Update artifact` mot et ikke-eksisterende artefakt «q3-rapport». Nøytral korreksjon sendt («Kontroller tallene i ledernotatet mot rapporttabellen og rett eventuelle avvik.»); modellen kjørte kodetolkeren igjen, rettet til 124 680 / +1 680 (v3) og beklaget. Endelig notat stemmer med kontrollfasit.
- Opptaksfil, råvarighet og redigert varighet: Ingen opptak (verktøyet har ikke skjermopptak). Stillbilder: oppgave sendt, Arbeid med kodetolker-utskrift, rapport i sidepanelet, ledernotat v3. Opptak må gjøres med opptaker i ny samtale med samme bundle (README steg 6); legg inn korreksjonsrunden bare hvis den oppstår igjen.
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Godkjent mot CONTROL-NOTES.md 02, 14.09.2026 (prøvekjøring; opptak gjenstår).

Produktfunn fra 02 forsøk 2 (til utvikling):
10. Sammendragstekster (chat-svar og ledernotat) gjentar ikke tall fra artefaktet/kodetolker-utskriften, men regner på nytt og bommer (1 360 vs. 1 680; 124 560 vs. 124 680; −0,2 vs. −2,0 pp). Tall som allerede finnes i et artefakt eller verktøyresultat bør gjenbrukes ordrett.
11. `Update artifact` ble kalt med et oppdiktet artefaktnavn («q3-rapport») som ikke finnes i samtalen; løkka lagde deretter et nytt artefakt og forsøkte «Reattach context» mot en melding som ikke finnes. Artefakt-ID-ene bør ligge i verktøykonteksten så modellen ikke må gjette.
12. Korreksjonssvaret ble merket «Usikkert svar (68 % sikkerhet) — ingen kilder ble brukt», selv om kodetolkeren nettopp hadde verifisert tallene. Sikkerhetsindikatoren teller tydeligvis bare kunnskapskilder, ikke verktøybevis.

## 03 Kampanje — forsøk 1 (prøvekjøring, godkjent med merknader)
- Oppgave og forsøksnummer: 03 Kampanje, forsøk 1, med oppfølging fra REGI.md.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (ingen Fjordform-arbeidsflate finnes).
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FHYT7WVHBXV1BJG1DM8S8X` (oppfølgingskjøring `01M2FJ4YXE76F8PYAZCDCEY95N`).
- Kilder brukt: innlimt `samtalestart.txt` ordrett (prompt + M1–M4). Ingen filer lagt ved. Modellen kjørte likevel `Knowledge search` og fikk treff på 01-kildepakken (indeksert som `chat-upload` fra 01 forsøk 1) og `aquatiq.com` (Kilder-fanen viste 1 kilde: aquatiq.com). Ingenting fra disse havnet i tekstene. «Brukte 5 minner fra tidligere samtaler».
- Modell og tilgjengelige verktøy: Claude Sonnet. Første tur: 6 206 tokens inn / 166 ut, 118,3 s, 88 % sikkerhet, $0,0211, 6 steg: `Knowledge search`, `Social list accounts` (0 tilkoblede kontoer — ingenting kunne publiseres), `Create artifact` ×3 (samme tittel, 3 615 / 3 535 / 3 864 tegn, ulikt innhold), `Reattach context` ×1 (feilet: «No earlier message … matches»). Oppfølging: 6 490 inn / 122 ut, 28,8 s, 86 %, 3 steg (`Reattach context` ×2 feilet, `Create artifact`). Kontekstvindu 94 % fullt etter oppfølgingen.
- Resultat som faktisk ble laget: Dokument «Fjordform — LinkedIn-utkast og e-post, 21.–25. september 2026» (siste versjon): status «UTKAST til gjennomgang. Ingenting er sendt, planlagt eller publisert», publiseringsoversikt (21.09 «Lyset som tilpasser seg dagen», 23.09 «Én lampe, mange arbeidsplasser», 25.09 «Detaljen som gjør kontoret ferdig», uke 39 e-post), tre innlegg, e-post med emnefelt/forhåndsvisning/brødtekst, «Intern merknad — kildebruk» som tabell påstand → kilde. Etter oppfølgingen: eget dokument «LinkedIn-utkast – 23. september 2026» (ca. 75 ord + intern merknad).
- Hvor resultatet finnes: samtalens Resultat-fane, fire dokumenter. Ingenting publisert, planlagt eller sendt.
- Avvik fra kontrollgrunnlaget: Kontrollpunktene i CONTROL-NOTES 03 er oppfylt: 21., 23. og 25. september ✔; tre ulike vinkler ✔; e-post med emnefelt og forhåndsvisningstekst ✔; produktfakta = M2 (tre lysnivåer/knapp på foten, justerbar arm, sand/grafitt, 16 cm, 2 års garanti, leveringstid bekreftes) ✔; pris 1 000 kr eks. mva ✔; ingen rabatt, «neste dag» eller kundeuttalelser ✔; ingen helse-/produktivitets-/miljøløfter, M4 eksplisitt utelatt ✔; bokmål, «dere», rolig tone ✔; tekstlig oppfordring, ingen URL ✔; kildehenvisninger i egen intern merknad ✔; status utkast ✔. **Merknader mot briefen (M1):** innlegg 1 er ca. 104 ord og innlegg 2 ca. 94 ord (brief: 60–90); e-postens brødtekst ca. 69 ord (brief: 80–120). Innlegg 3 sier lampen «veier lett nok» — vekt står ikke i M2. Én av de tre likt navngitte versjonene skriver «Pris fra 1 000 kroner» («fra» er unøyaktig). Chat-svarets oversiktstabell bruker andre vinkeltitler enn dokumentet i sidepanelet. Emneknagger vises som «\#arbeidsplass» (escapet markdown) i sidepanelet.
- Oppfølging og endring: «Gjør innlegget for 23. september mer konkret for et kontor som deles av flere team. Behold den rolige tonen og bruk bare dokumenterte produktegenskaper.» sendt. Resultat: nytt innlegg med vinkel «Justerbar arm i et kontor delt av flere team», ca. 75 ord, kun M2-egenskaper (justerbar arm), rolig tone, to emneknagger, oppfordring «Be om produktarket». Godkjent. Ingen ny korreksjonsrunde.
- Opptaksfil, råvarighet og redigert varighet: Ingen opptak (verktøyet har ikke skjermopptak). Stillbilder: oppgave sendt, Arbeid-fanen, dokument i sidepanelet, revidert 23.09-innlegg. Opptak må gjøres med opptaker i ny samtale med samme bundle (README steg 6).
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Godkjent mot CONTROL-NOTES.md 03 med merknader om ordlengde, 14.09.2026 (prøvekjøring; opptak gjenstår).

Produktfunn fra 03 (til utvikling):
13. Kunnskapssøk hentet inn en tidligere demo-opplasting (01-kildepakken) og aquatiq.com i en ny, urelatert samtale. Filer lastet opp i chat blir varig indeksert i org-kunnskapen uten at brukeren ble spurt; demoen bør kjøres i en ren arbeidsflate eller opplastingene bør ryddes.
14. Tre `Create artifact` med identisk tittel i én tur ga tre dokumenter med avvikende tekst (bl.a. «Pris fra …» i én). Brukeren kan ikke se hvilken som er «endelig»; chat-svaret oppsummerte en annen versjon enn den siste.
15. `Reattach context` feilet tre ganger i denne tråden («No earlier message … matches») — modellen ber om kontekst som ikke finnes. Kontekstvinduet sto på 94 % etter to turer med 4 096-tokens budsjett; lengre demoer vil treffe komprimering.
16. Ordgrenser fra briefen (60–90 / 80–120 ord) ble ikke holdt i den første versjonen, selv om de sto eksplisitt i M1.
17. Markdown-escapede emneknagger («\#») lekker som synlige tegn i dokumentvisningen.

## 04 Prosjektplan — forsøk 1 (prøvekjøring, godkjent)
- Oppgave og forsøksnummer: 04 Prosjektplan, forsøk 1, med oppfølging fra REGI.md.
- Dato og demoarbeidsflate: 14. september 2026. Arbeidsflate AQUATIQ AS (ingen Fjordform-arbeidsflate finnes).
- Faktisk samtalereferanse / kjørings-ID: tråd `01M2FJ9R9WV1H9GXPNBP5DSV08` (oppfølgingskjøring `01M2FJEESDWREY6T972G3805N2`).
- Kilder brukt: innlimt `samtalestart.txt` ordrett (prompt + P1–P3). Ingen filer lagt ved. Ingen Kilder-fane/eksterne treff. «Brukte 5 minner fra tidligere samtaler»; ingenting fra Aquatiq i teksten.
- Modell og tilgjengelige verktøy: Claude Sonnet. Første tur: 5 072 tokens inn / 491 ut, 93,0 s, 84 % sikkerhet, $0,0226, 2 steg: `Code interpreter` (arbeidsdagsberegning T1–T8) + `Create artifact`. Oppfølging: 6 163 inn / 60 ut, 15,2 s, 82 %, 1 steg (`Create artifact`). Ingen feilede steg.
- Resultat som faktisk ble laget: Dokument «Prosjektplan — Kundeportalpilot (utkast)» (4 976 tegn): varsel «Ingen kalenderavtaler er opprettet og ingen varsler er sendt», oppgavetabell T1–T8 med ansvarlig rolle, frist (merket vedtatt/beregnet/forslag), avhengigheter og ferdigkriterium, «Realistisk rekkefølge» dag for dag, Konflikt 1 (invitasjonsdato), Risiko 1–3, tabell «Skille: vedtatte datoer vs. forslag». Etter oppfølgingen: «Intern status — kundeportalpilot» (75 ord).
- Hvor resultatet finnes: samtalens Resultat-fane, to dokumenter. Ingen møter opprettet, ingen varsler sendt.
- Avvik fra kontrollgrunnlaget: Rekkefølgen er identisk med CONTROL-NOTES 04: skjema 16.09 ✔, eksport 17.–18.09 ✔, veiledning 21.09 ✔, brukertest 22.–23.09 ✔, invitasjoner tidligst 24.09 «forutsatt ingen blokkerende feil» ✔, lanseringsgjennomgang 28.09 ✔, beslutning 30.09 ✔. 22.09 omtalt som salgsansvarligs forslag og «ikke mulig» ✔; 24.09 merket «beregnet minimum», ikke bekreftet ✔; teknisk ansvarlig borte 21.–23.09 som Risiko 1 med feilretting som konsekvens ✔; anbefaling «avklar stedfortreder», ingen oppdiktet person ✔; rolle/frist/avhengighet/ferdigkriterium på alle oppgaver ✔; ingen møter/varsler ✔. Merknader: i worst-case-avsnittet antar modellen én dag retting og én dag kontroll («retting 24., ny kontroll 25., invitasjoner 26.») — varigheter som ikke finnes i grunnlaget; det er merket som konsekvensbeskrivelse, ikke plan, men tallene er oppdiktet. Chat-svaret og statusnotatet sier invitasjoner «forskyves til 28. september» mens plandokumentet sier 26. — intern inkonsistens. T5 (invitasjonsutkast) fikk 22.09 som frist med etiketten «forslag»; i notatet var 22.09 salgsansvarligs forslag om utsendelse, ikke utkast.
- Oppfølging og endring: «Lag en kort intern status til prosjektleder med de to viktigste risikoene og beslutningen som må tas først. Maks 100 ord. Ikke send den.» sendt. Resultat: 75 ord, Risiko 1 teknisk utilgjengelighet, Risiko 2 invitasjonsdato, «Beslutning som haster: Hvem er teknisk stedfortreder 21.–23. september? Avklar i dag.» Ikke sendt til noen. Godkjent.
- Opptaksfil, råvarighet og redigert varighet: Ingen opptak (verktøyet har ikke skjermopptak). Stillbilder: oppgave sendt, Arbeid med datoberegning, plan i sidepanelet, statusnotat. Opptak må gjøres med opptaker i ny samtale med samme bundle (README steg 6).
- Tidskutt/fremskynding: —
- Poster og undertekst: —
- Kontrollør og godkjent dato: Godkjent mot CONTROL-NOTES.md 04, 14.09.2026 (prøvekjøring; opptak gjenstår).

Produktfunn fra 04 (til utvikling):
18. Chat-svar og oppfølgingsnotat gjengir ikke plandokumentets tall (26. vs. 28. september) — samme mønster som funn 10. Sammendrag bør genereres fra artefaktet, ikke fra minnet om det.
19. Kodetolkeren gjorde datoberegningen riktig og raskt (2 steg, ingen feil) når den var tilgjengelig — kontrasten til 02 forsøk 1 viser hvor mye kvaliteten avhenger av at verktøyet faktisk kan kalles.

## Samlet status 14.09.2026
- Alle fire oppgaver er prøvekjørt med innlimt `samtalestart.txt` (README-reserven) fordi vedlegg i samme tur nås avkortet (funn 1). Alle fire er godkjent mot CONTROL-NOTES.md; 02 og 03 med merknader (ledernotat trengte korreksjon; ordlengder i 03).
- Opptak gjenstår for alle fire og må gjøres med en ekstern opptaker; verktøyet kan ikke ta skjermopptak. Bruk nye samtaler og nøyaktig samme bundle. Vurder å rydde 01-opplastingen fra org-kunnskapen først (funn 13) og å kjøre i en arbeidsflate uten Aquatiq-kunnskap.
- Driftsfeil rettet underveis: capability-core manglet signeringsnøkkel → kodetolkeren ble avvist (02 forsøk 1). Gjenskapt via launcheren; verifisert i 02 forsøk 2, 03 og 04.
- Produktfunn 1–19 er samlet i denne loggen for utvikling; ingen av dem er løst i kode i dag.

## Rettelser i kode etter funn 1–19 (14.09.2026, ettermiddag)
Status per funn — «rettet» betyr endret i kode og typesjekket/testet lokalt; verifisering mot live kjøring står under «Verifisering».
| Funn | Rettelse | Hvor |
| --- | --- | --- |
| 1 | Små tekstvedlegg (≤ 60 000 tegn, ≤ 120 000 samlet) sendes nå med i selve meldingen til modellen («--- VEDLEGG: navn ---»), i tillegg til at chip og visning er uendret. Større filer går som før via indeksering. | SPA `use-chat-controller.ts`, `chat-normalizers.ts`, `chat-types.ts` |
| 2, 5, 14 | `create_artifact` med en tittel som allerede finnes i samtalen behandles som oppdatering av samme artefakt (ny versjon), ikke et nytt dokument. Tittel-indeks per tråd. Verktøybeskrivelsen sier det samme. | model-gateway `artifacts.rs`, `tool_loop.rs` |
| 3 | Artefakter (dokument/kode/html, siste versjon per id) lagres i assistentmeldingens metadata og relayes i trådlesingen, så Resultat-panelet fylles på nytt ved gjenåpning. | model-gateway `sse_events.rs`, `session_flow.rs`, `sse.rs`; BFF `history.rs` |
| 6, 8 | Arbeid-fanen viser «Feilet: <steg> — <årsak>» i stedet for «Siste steg feilet». execution-core navngir årsaken: «capability policy misconfigured: … (deployment fault)» / «unreachable» / vanlig avslag, og ber modellen si det til brukeren. | SPA `ChatPanels.tsx`; execution-core `runtime_loop/mod.rs` |
| 7, 10, 16, 18 | Fast systemkontekst «Response discipline»: gjenbruk tall/datoer ordrett fra artefakt/verktøyresultat, hold eksplisitte grenser (ordtall), si fra når et verktøy feiler og ikke erstatt det med hoderegning, ett artefakt per arbeidsprodukt. | model-gateway `tool_loop.rs` (`RESPONSE_DISCIPLINE_NOTICE`), `sse.rs` |
| 9 | capability-core nekter å starte uten `CAPABILITY_CORE_DECISION_SIGNING_KEY` (eksplisitt `CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS=true` for usignert utviklingskjøring). | capability-core `cmd/main.go` |
| 11 | `update_artifact` med ukjent id lister artefaktene som finnes (id + tittel). Eksempel-id «q3-rapport» er fjernet fra beskrivelsen (modellen kopierte den). | model-gateway `tool_loop.rs` |
| 12 | Sikkerhetsmerknaden regner vellykkede verktøykall som bevis; «ingen kilder ble brukt» vises ikke lenger under et svar kodetolkeren har verifisert. | SPA `shared/chat-nodes/derive.ts` |
| 13 | Vedlegg som sendes med i meldingen indekseres ikke lenger automatisk i org-kunnskapen (melding sier det, og peker på «Legg i kunnskapsbasen»). Allerede indekserte demo-opplastinger (01-kildepakken) må fjernes manuelt i Kunnskap. | SPA `use-chat-controller.ts` |
| 15 | `reattach_context` tilbys bare når historikken faktisk er komprimert (markør fra komprimeringen); beskrivelsen sier at den returnerer ingenting ellers. | model-gateway `tool_loop.rs`, `sse.rs` |
| 17 | Markdown-escapes (`\#`, `\*`, `\_` …) vises som det escapede tegnet. | SPA `chat-media-markdown.tsx` (+ tester) |
| 3b (nytt) | Artefaktlageret i model-gateway er prosesslokalt og ble tomt ved omstart, så `update_artifact` feilet i alle åpne samtaler etter et deploy. Lageret gjenoppbygges nå fra artefaktene som er lagret i trådens metadata når samtalen lastes. | model-gateway `artifacts.rs` (`seed_persisted`), `sse.rs` (`rehydrate_persisted_artifacts`) |
| 5b (nytt) | Utelatt tittel ved `update_artifact` faller tilbake til artefaktets eksisterende tittel (viste «document» i panelet); fra v3 sier verktøysvaret «svar brukeren i stedet for å oppdatere igjen». Nesten-riktige id-er («salgsrapport-uke-37» → «salgsrapport-uke37») løses når treffet er entydig. | model-gateway `tool_loop.rs`, `artifacts.rs` |
| 20 (nytt) | **Modellen kunne ikke lese sitt eget artefakt.** Skriveverktøyene gir bare en bekreftelseslinje tilbake (så en revisjon ikke limer 4 000 tegn inn i samtalen), så en senere tur hadde ikke dokumentteksten i kontekst. Resultat: ledernotatet skrev 127 000 kr der rapporten sa 123 000. Nytt verktøy `read_artifact` gir teksten tilbake på forespørsel; lageret tar vare på innholdet og gjenoppbygges fra trådens metadata etter omstart. | model-gateway `tool_loop.rs`, `artifacts.rs`, `sse.rs` |
| 21 (nytt) | `read_artifact` uten id er en opplisting, ikke en feil — den ble vist som «1 feilet» i Arbeid-fanen i hver kjøring. | model-gateway `tool_loop.rs` |
| 18b | Chat-sammendraget navnga en annen «første oppfølging» enn dokumentet. Fast systemkontekst sier nå at et sammendrag skal gjengi artefaktets egne punkter, eiere og rekkefølge, og verktøysvaret peker på `read_artifact`. | model-gateway `tool_loop.rs` |
| 4 | Ikke rettet: egen «kun vedlegg»-modus er et produktvalg; med funn 1 rettet får modellen vedlegget i kontekst, men kunnskapssøk kjører fortsatt. | — |
| 19 | Ikke et avvik. | — |

Verifisering (14.09, etter ombygging med `build-verevon-services.sh --from model`; alle containere friske):
- Lokale sjekker: Go build + tester (capability-core), `cargo check` + 54 (model-gateway) / 109 (execution-core) / 11 (BFF history) tester, `tsc -b`, 353 vitest — alle grønne.
- Ny prøvekjøring av 02 med **ekte vedlegg** (kildepakke.md + salg.csv via filvelgeren), tråd `01M2FTFQTH1N3C0XS161N8B953`: vedleggene nådde modellen i samme tur (funn 1 ✔; melding «ble sendt med i meldingen. Ikke lagret i kunnskapsbasen» = funn 13 ✔); kodetolkeren kjørte (funn 9 ✔); ett artefakt, 2 steg; rapporttallene og chat-sammendraget stemmer med CONTROL-NOTES (funn 10 ✔); den varige trådlesingen bærer nå artefaktet (`salgsrapport-uke37@v1`, funn 3 ✔); brukerturen bærer «--- VEDLEGG: kildepakke.md ---».
- Oppfølging «Kok dette ned …»: ledernotat med riktige tall (124 680 / +1 680) — ingen korreksjonsrunde nødvendig (funn 10/18 ✔). Chat-svaret og notatet nevnte imidlertid ulik «første oppfølging» (markedsansvarlig vs. salgsleder) — funn 18 delvis igjen (tekst, ikke tall).
- Revisjon «Gjør sammendraget kortere, maks 50 ord»: ETT artefakt oppdatert til v7, ingen duplikater (funn 2/5/14 ✔), sammendrag på 41 ord, modellen talte ordene i kodetolkeren (funn 16 ✔). Rest: seks oppdateringer for én liten endring (churn), og panelet viste tittelen «document» fordi modellen utelot tittel ved oppdatering. Begge rettet i kode etterpå (tittel-fallback; «svar brukeren i stedet for å oppdatere igjen» fra v3) og krever ny model-gateway-image.
- Nesten-riktig id (`salgsrapport-uke-37` for `salgsrapport-uke37`) skjedde to ganger; den nye feilmeldingen listet riktig id (funn 11 ✔), og normalisert id-oppslag er lagt til i kode etterpå (krever ny image).
- Gjenåpnet tråd 03 (`01M2FHYT7WVHBXV1BJG1DM8S8X`): emneknaggene vises som «#arbeidsplass #kontorinnredning» uten omvendt skråstrek (funn 17 ✔). Gjenåpnet tråd 02 forsøk 2: merknaden på korreksjonsturen lyder nå «Usikkert svar (68 % sikkerhet) — sjekk kildene før du stoler på dette» i stedet for «ingen kilder ble brukt» (funn 12 ✔).
- Etter ny model-gateway-image og omstart (kl. 13:49): revisjon «Legg til en linje nederst …» på samme tråd ga `Update artifact 'Salgsrapport uke 37 vs. uke 36 — Fjordform' (v8)` i 2 steg — lageret ble gjenoppbygd fra metadata (versjonen fortsatte fra v7), tittelen er tilbake i panelet («8 versjoner»), linjen ligger nederst i dokumentet, ingen churn. Før rettelsen svarte modellen på samme forespørsel «Jeg har ikke et aktivt artefakt å redigere».
- Kunnskapssøk hentet fortsatt inn den gamle 01-opplastingen (dokument-id `b93e5cd2-…`) — den må slettes manuelt i Kunnskap; nye vedlegg indekseres ikke lenger automatisk.

## Full prøvekjøring av alle fire oppgaver på ferdig bygg (14.09, kl. 16:0x–16:3x)
Kjørt med **ekte vedlegg** fra manifest (filvelgeren), én ny samtale per oppgave, REGI-oppfølging etter hver. Alle containere friske; model-gateway bygget med funn 1–21 rettet.

| Oppgave | Tråd | Steg | Artefakter | Kontroll mot CONTROL-NOTES |
| --- | --- | --- | --- | --- |
| 01 Kundesvar | Kjørt 14.09 (3 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G70YVQVYHGHN6AER01VVRZ` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 02 Salgsrapport | Kjørt 14.09 (3 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G75EFP4NZDS2S0J4P40FC3` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 03 Kampanje | Kjørt 14.09 (2 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G7BB4X7CK7PGSYVVE0DZNA` | Ikke mulig fra verktøyet (kun stillbilder) | — |
| 04 Prosjektplan | Kjørt 14.09 (2 forsøk; siste på ferdig bygg) | Godkjent mot kontrollfasit | `01M2G7G9Q2AVB3JAW7RW5BV715` | Ikke mulig fra verktøyet (kun stillbilder) | — |

- **01**: 9 av 12 pakket, 3 på påfyll ✔; 17.09 som ankomst til eget lager ✔; «transportøren har ennå ikke hentet kolli» ✔; ingen leveringsgaranti ✔; dellevering undersøkes, tid og frakt bekreftes før booking ✔; K3 brukt, K4 eksplisitt utgått ✔; ny status senest 15.09 ✔; utkast + intern kildetabell adskilt ✔. Oppfølging: 82 ord (≤100), varmere, usikkerheten og kildetabellen beholdt ✔ — **ett** artefakt i v2.
- **02**: 360 000/360 000, 237 000/235 320, 123 000/124 680, 34,17 %/34,63 % (+0,47 pp) ✔; per gruppe +25 %/+40 %/−30 % med margin 40/35/30→28 ✔; 48 000 holdt utenfor og begrunnet ✔; fakta skilt fra ubekreftede årsaker ✔. Ledernotat: **94 ord, 124 680 kr, 34,6 %, −2,0 pp — alle tall identiske med rapporten** (funn 20 borte; forrige kjøring ga 127 000/128 680).
- **03**: tre innlegg på **69/73/75 ord** (brief 60–90, forrige kjøring 104/94/87) ✔; e-post med emnefelt, forhåndsvisning og brødtekst ✔; kun M2-fakta, pris 1 000 kr eks. mva ✔; M4-påstandene eksplisitt utelatt ✔; to emneknagger, ingen URL, ingen omvendt skråstrek ✔; **ett** artefakt (forrige kjøring: tre med samme tittel). Oppfølging: 23.09-innlegget omskrevet til delt kontor **i samme dokument (v2)**, de tre andre tekstene uendret ✔.
- **04**: rekkefølge identisk med kontrollfasit (16 → 17–18 → 21 → 22–23 → tidligst 24 → 28 → 30) ✔; hver dato merket [VEDTATT] eller [FORSLAG] ✔; 22.09 avvist som ikke gjennomførbar ✔; teknisk fravær 21.–23. som kritisk risiko, ingen oppdiktet stedfortreder ✔; rolle/frist/avhengighet/ferdigkriterium på alle T1–T8 ✔; ingen møter eller varsler ✔. Oppfølging: intern status på **81 ord** med de to risikoene og beslutningen først, «Utkast — ikke sendt» ✔.
- Gjennomgående: modellen kaller nå `read_artifact` før den oppsummerer eller reviderer, og chat-sammendraget stemmer med dokumentet i alle fire kjøringene.

### Etterkontroll av funn 21 (kl. 17:2x)
Ny 02-kjøring etter siste bygg: 2 steg, **ingen «feilet»-merke** i Arbeid-fanen, og oppfølgingen leste artefaktet direkte (`Read artifact -> Current content of artifact 'salgsrapport-uke-37' (v1)`). Ledernotat: 103 ord, 124 680 kr / +1 680 / 34,6 % / −2,0 pp — identisk med rapporten.

### Funn 22–25 (14.09, kveld) — minne og kunnskapsbase
Undersøkelsen av «minnelekkasjen» i 04 avdekket fire separate feil, tre av dem alvorligere enn symptomet:

| Funn | Hva som er galt | Rettelse |
| --- | --- | --- |
| 22 | **Demodata ble lagret som fakta om personen.** Minneuttrekket (`dream_extractor`) leser hele brukerturen, inkludert vedlagt/innlimt dokumenttekst, og konkluderte om brukeren ut fra kildepakkene. Etter fire demokjøringer sto det bl.a. «User works at Fjordform.» (feil — brukeren jobber i Aquatiq), «User is involved with a customer portal pilot project scheduled for 30 September 2026.» og «Campaign targets office managers…» i det varige minnet — 36 av 43 lagrede «fakta» kom fra demopakkene. | Vedleggs- og kildeblokker (`--- VEDLEGG: … ---`, `--- KILDE: … ---`) fjernes fra brukerturen før uttrekk; en tur som bare er et vedlegg faller helt ut. Prompten sier nå at materiale brukeren leverer er *arbeidets tema*, ikke en beskrivelse av personen, og at «vi/vår» om et dokument ikke gjør innholdet sant om brukeren. session-core `dream_extractor.rs` |
| 23 | **Gjenkalte minner ble presentert som relevante fakta.** Blokken åpnet med «Relevant memory:», og modellen skrev dem inn i leveranser — i 04 havnet «Ekstra frakt ved dellevering (ordre FF-1042)» fra en annen samtale i prosjektplanen. | Ny fast innledning: dette er bakgrunn om personen, kan være irrelevant, skal brukes til språk/tone/preferanser — ikke som kilde, og ikke inn i dokument, plan eller rapport med mindre brukerens forespørsel eller materiale viser til det. Delt hjelpefunksjon så SSE- og gRPC-veien ikke kan skille lag. model-gateway `memory_provenance.rs`, `sse.rs`, `grpc.rs` |
| 24 | **Minner kunne vises, men ikke slettes.** «Vis hva jeg husker» slår sammen den varige tabellen og den semantiske (Letta), men `DeleteMemory` returnerte `not_found` så snart den varige raden manglet — uten å spørre den semantiske. 7 av 43 rader var derfor synlige og permanent uslettbare (DSAR-relevant). | Den semantiske sletten forsøkes alltid; `not_found` bare når ingen av lagene hadde raden. session-core `memory_grpc.rs` |
| 25 | **Kunnskapsbasen var append-only fra produktet.** Et chat-vedlegg blir et varig, org-bredt dokument, men verken SPA eller BFF hadde noen slettevei — `DELETE /api/v1/knowledge/documents/{id}` fantes ikke (405), selv om documents-api har støttet `DELETE /v1/documents/{id}` hele tiden. Derfor lå demo-opplastingen fra første 01-forsøk igjen og traff kunnskapssøk i senere kjøringer. | Ruten lagt til i BFF med samme scopede Data Plane-legg som opprettelse. Frontend Plane `domains/knowledge.rs`, `knowledge/documents.rs` |

**Etterkontroll (kl. 17:4x, på nytt bygg av session-core, model-gateway og BFF):**
- Funn 24: de 7 fastlåste minnene lot seg slette — minnelageret er nå tomt (0 av 43).
- Funn 25: `DELETE /api/v1/knowledge/documents/{id}` svarer nå, og demo-dokumentet er borte. Kunnskapsbasen inneholder bare de tre ekte Aquatiq-kildene (skills-list.pdf, to aquatiq.com-sider).
- Funn 22: oppgave 04 kjørt på nytt med ekte vedlegg. session-core logget «dreaming extraction produced no storable memory» for samtalen — **ingen nye påstander om brukeren**. Før rettelsen ga samme samtale «User is involved with a customer portal pilot project scheduled for 30 September 2026» og «User is managing and working on a customer portal pilot project for Fjordform».
- Funn 23: planen fra samme kjøring inneholder ikke FF-1042, dellevering, frakt eller lamper — avklaringspunktene handler utelukkende om piloten. 24. september står fortsatt som tidligste invitasjonsdato, så kontrollfasiten holder.

Opprydding utført etter avklaring med bruker: alle 43 minner slettet (36 varige med en gang; de 7 semantiske etter funn 24-rettelsen), og demo-dokumentet `b93e5cd2-…` (`kildepakke.md`, `chat-upload`) fjernet fra kunnskapsbasen. De tre ekte Aquatiq-dokumentene er urørt.

### Funn 26–29 (14.09, kveld) — driftsfeil funnet ved systemgjennomgang
Etter minne- og kunnskapsrettelsene gikk jeg gjennom resten av systemet (feiltellinger per container, deretter full revisjon av alle NATS-identiteter). Tre levende feil, ingen av dem synlige i grensesnittet:

| Funn | Hva som var galt | Rettelse |
| --- | --- | --- |
| 26 | **GDPR-slettekanalen var død.** `session-core` feilet NATS-autentisering mot control-shared-nats ~240 ganger i minuttet (2 363 feil på 45 min). Årsak: `SESSION_CORE_GDPR_NATS_PASSWORD` fantes i BÅDE Control Plane (eieren — control-shared-nats autoriserer brukeren derfra) og `Model Plane/deploy/.env`, og de hadde driftet. Launcheren laster plane-filen sist, så den utdaterte kopien vant for session-core. | Duplikatet fjernet fra Model Plane med en forklarende merknad; Control er eneste eier. GDPR-konsumenten er nå tilkoblet og mottar meldinger. |
| 27 | **Revisjonskanalen var død.** Samme drift motsatt vei: `audit-core-model` (en Control-container) presenterte Controls verdi, mens model-plane-nats autoriserer fra `Model Plane/deploy/.env`. | Controls speilkopi justert til Model Planes verdi, med merknad om hvem som eier den. |
| 28 | **Ingen kunne logge inn.** `/api/v1/auth/session` svarte 500. `auth-core/.env.docker` hardkoder Dragonfly-passordet **inne i en URL**, og literalen fulgte aldri med da passordet ble rotert: Dragonfly krevde 64-tegnsverdien, auth-core presenterte en 48-tegns. Feilen lå latent i tre døgn — den ble først synlig da Dragonfly ble gjenskapt, mens auth-service fortsatt kjørte på gammel env. | `DRAGONFLY_URL` settes nå i `docker-compose.yml` fra `${DRAGONFLY_PASSWORD}` (samme variabel som Dragonflys egen `--requirepass`), så de to ikke kan avvike. Begge literalene justert. |
| 29 | Byggeveien var ikke dokumentert der man trenger den: et håndsatt `docker compose build` mangler Control Planes env-kjede og feiler eller starter mot drifted config. | Merknad øverst i `Model Plane/deploy/docker-compose.yml` og `Frontend Plane/verevonv3/docker-compose.yml` om at `build-verevon-services.sh` er eneste riktige vei. |

**Etterkontroll:** full revisjon av alle NATS-identiteter — **31 bindinger, null avvik** (var 2). Null autentiseringsfeil på session-core, control-shared-nats og model-plane-nats de siste minuttene. Innlogging virker igjen (session 200), ingen containere er unhealthy.

**Kunnskap-siden har nå en sletteknapp** (funn 25s manglende halvdel): «Slett fra kunnskapsbasen» i utdragspanelet, med bekreftelse som navngir dokumentet, varsel og oppfriskning av listen. Verifisert i grensesnittet mot de tre ekte Aquatiq-kildene.

### Gjenstår (ikke kodefeil)
1. **Opptak**: fortsatt ikke mulig fra dette verktøyet. Bundlene og promptene over er verifisert og kan brukes som de er.
2. **Ingen sletteknapp i grensesnittet**: BFF-ruten finnes nå (funn 25), men Kunnskap-siden har ingen knapp som kaller den. Neste steg hvis dette skal være selvbetjent.


## 19.09.2026 — Q01–Q04: nytt akseptansegrunnlag og rettelser

**Status:** Q01–Q04 er implementert og verifisert i lokal stakk. Siste vedlikeholdte nettleserkjøring (`live-10`) bestod **6 av 6 tester**. Dette er ikke ny godkjenning av alle fire scenarioer eller klart markedsmateriell. Full dokumentasjon, også av mislykkede forsøk: [Q01–Q04 execution record](evidence/PRODUCT_RECORDING_Q01_Q04_2026-09-19.md).

- Q01: innlogget testbruker, aktiv organisasjon, fullført onboarding og HTTP-sjekk før chat. Vite oppdaget tidligere filer og overvåket store, uvedkommende mapper; oppstarten kunne stoppe i flere minutter. Avgrenset skanning/overvåking og erstattet TCP-helsesjekk med HTTP.
- Q02: PDF, DOCX med tabell, CSV og Markdown er faktisk lest gjennom både dashboard og chat. Ekstraksjonsfeil og fullt nettleserlager beholder utkast og filer og starter ingen inferens. Vedlegg blir ikke automatisk organisasjonskunnskap. Dokumentekstraksjon er midlertidig; etter servergjenåpning er PDF/DOCX tilgjengelig som ærlig navngitt tekstkopi.
- Q03: lokal reservekomprimering og native checkpoint føres gjennom kontrakt, lagring og neste forespørsel. En reell Sonnet-samtale passerte 159 062 inputtokens; etter omstart av begge Model-tjenestene brukte oppfølgingen 1 911 inputtokens og beholdt ordrefakta, pin og testmarkør. Native-av/andre ruter og ZDR er kontrakttestet; dette er ikke livebevis for alle leverandører.
- Q04: uendrede filer for scenario 01 og nøyaktig REGI-oppfølging. Rettet feil ukedag/scenarioklokke, oppdiktede artefakt-ID-er, rå vedleggstekst ved gjenåpning, sitert Markdown-tabell og tapt versjonsvelger. Siste kjøring reviderte `svar-ff-1042` fra v1 til v2 og åpnet kilde og begge versjoner etter tømming av lokal transkriptbuffer. En separat gateway-omstart fulgt av ny nettleser og revisjon ga v2 til v3 på samme artefakt, uten feilede verktøysteg.

Siste scenario: ca. **43 sekunder** for første utkast og **29 sekunder** for revisjonen. Ingen meldinger sendt eller ordre endret. Kontrollfasiten ble gjennomgått: 9 pakket / 3 venter, 17. september gjelder foreløpig lagerankomst, ingen bekreftet kundelevering, oppfølging 15. september, K3 gjelder og K4 er utgått.

**Neste:** Q05 med scenario 02–04 og oppfølginger, deretter minne-/kildeisolasjon, bredere ytelses- og gjenopprettingstester og visuell sluttkontroll. Fem sammenhengende ferske beståtte samtaler per scenario er fortsatt opptakskrav; en grønn kjøring erstatter ikke det. Rå nettlesertracer kan inneholde sesjonshoder og skal ikke brukes som markedsføringsfiler.

## 23.09.2026 — Gjeldende opptaksstatus etter pass 4

Avsnittene over er historiske kjørelogger fra 14.–19. september. «Godkjent» der betyr kontroll av det konkrete eldre forsøket, ikke godkjenning av et publiseringsbygg eller dagens produktfilmer. Alle nye live-tester i [pass 4](evidence/PRODUCT_RECORDING_CAPTURE_PASS4_2026-09-23.md) brukte bare `gpt-5.6-terra` via `openai-codex-subscription`; eldre logglinjer om Claude er ikke den aktive opptaksruten.

| Oppgave | Nåværende bevis | Gjenstår |
| --- | --- | --- |
| 01 Kundesvar | Én bestått privat prøvefilm fra et eldre bygg | Fem sammenhengende kvalifiserte fulle kjøringer på valgt bygg og øvrige publiseringsporter |
| 02 Salgsrapport | Én bestått privat prøvefilm fra et annet bygg | Samme krav; kontroller tall i både chat, artefakt og oppfølging |
| 03 Kampanje | Én bestått privat prøvefilm med REGI-oppfølging og gjenåpning | Samme krav; kontroller ordgrenser og dokumenterte påstander hver gang |
| 04 Prosjektplan | Ingen fullført bestått opptak i siste pass. Nye normale forsøk traff tidsavbrudd etter kildekontroll og reparasjon | Kildebundet førsteutkast og en uavhengig recheck innen eksisterende frist; deretter komplett oppfølging, gjenåpning og privat film |

Alle fire står på **0/5** kvalifiserte sammenhengende kjøringer; 20/20 offentlige mediefelt er tomme. Høyinnsats-kildekontrollen bestod den forrige komplette 104/104-matrisen. Et raskere forsøk feilet én gyldig prosjektdato (103/104) og ble forkastet. Forsøk, feilsituasjoner og teknisk bevis er bevart privat; ingen feilkjøring er ommerket til bestått. [README](README.md) og [opptaksprosedyren](RECORDING-WORKFLOW.md) angir prioritert arbeid og publiseringsporter.

## 23.09.2026 — Pass 5: prioritert abonnementskontroll og prosjektforsøk v21–v25

Dette er gjeldende status etter [pass-5-rapporten](evidence/PRODUCT_RECORDING_CAPTURE_PASS5_2026-09-23.md). Alle nye live-kall brukte bare `gpt-5.6-terra` via `openai-codex-subscription`. Kildekontrollen beholdt høy resonneringsinnsats og ba abonnementstjenesten om prioritert behandling. Den bestod 104/104 merkede semantiske tilfeller og 10/10 uavhengige recheck-kontroller.

Prosjektforsøk v21–v25 ga fortsatt ingen komplett bestått førstetur, eksakt REGI-revisjon, gjenåpning eller film. v22 og v24 opprettet kildekontrollerte artefakter, men hele nettleserløpet feilet. v24 avdekket en falsk avvisning i testorakelet av en foreslått dato med tydelig godkjenningsvilkår; denne er rettet. v25s uavhengige recheck fant ytterligere udokumentert innhold, og videre retting rakk ikke fristen. Den raske komprimerte rechecken fra v21 ble forkastet; fullstendig kildegrunnlag og uavhengig kontroll er beholdt.

Alle fire oppgaver står fortsatt på **0/5** sammenhengende kvalifiserte kjøringer på samme publiseringsbygg. **20/20** offentlige mediefelt er tomme, og publisering er blokkert. Neste arbeid er å gjøre første prosjektutkast kildebundet, få hele dokumentet gjennom uavhengig recheck innen eksisterende frist, og deretter bevise full revisjon/gjenåpning før nye filmopptak og femløpsserier.

## 23.09.2026 — Pass 6: separat prosjektstatus og privat prøvefilm

Prosjektløpet har nå flere komplette private kjøringer med kildekontrollert prosjektplan, en **egen** intern REGI-statusnote og begge artefakter bevart etter gjenåpning. v30 viste hvorfor en grønn test uten bevaringskontroll ikke var nok: statusnoten overskrev da planens nyeste versjon. Den skjerpede testen avviser dette, og senere beståtte kjøringer bevarer begge. Den firekolonners planen er lesbar ved 1440 × 900. Et bestått v57-råopptak på **137,88 sekunder** fra det siste bygg er klippet til en **34,52-sekunders** privat prøvefilm med norsk tekstspor og tydelig merking av tidskutt; råopptaket er bevart. Det eldre v41-klippet er bevart som historisk prøve. Ingen medier er flyttet til offentlig mappe.

Feilforsøkene er like viktige for status: v34, v36, v40, v42, v45 og v48 viste tidsavbrudd etter nye udokumenterte påstander eller gjentatt note-reparasjon. v47 og v52 ble avvist av for snevre norske nettleserfasiter; disse er rettet, men feilopptakene står fortsatt som feil. v52 inneholdt også en faktisk udokumentert «vedtatt»-merking av en oppgitt aktivitetsdato; lokal kildekontroll og forfatterveiledning er skjerpet. [Pass-6-rapporten](evidence/PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md) skiller hvert bygg og utfall. Alle live-kall brukte bare `gpt-5.6-terra` via `openai-codex-subscription`, uten automatisk omkjøring.

På siste gateway-image `sha256:d5c2ceba6f568d1c018984032cda28cd4bed0a77668830ea0f3883bce91ed9c6` bestod v53–v57 **5/5 sammenhengende private** prosjektforløp med uendrede kilder. Første plan tok 44,0–116,0 sekunder; intern status 29,3–45,5 sekunder. Dette er sterkere privat korrekthetsbevis, men ikke den kvalifiserte publiseringsserien for alle fire oppgaver eller en etablert p95-ytelse.

Publiseringsstatus er fortsatt **0/5** kvalifiserte sammenhengende kjøringer per scenario og **20/20** tomme offentlige mediefelt. Nytt arbeid er å bevise gjentatt stabilitet på ett valgt bygg, deretter fullføre ytelse, gjenoppretting, tilgjengelighet, faglig/visuell filmkontroll og nettsidekontroll.
