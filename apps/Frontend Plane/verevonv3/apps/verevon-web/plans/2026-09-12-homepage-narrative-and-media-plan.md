# Verevon: plan for historie, bilder og produktbevis på forsiden

Dato: 12. september 2026  
Status: Første gjennomføringspass startet 12. september 2026. Heroens anbefalte overskrift og ingress er implementert i eksisterende layout. Dagens film står som midlertidig materiale mens nye kandidater vurderes. Bevegelsesvalget og øvrige faser er ikke implementert ennå.

## 1. Retningen jeg anbefaler

Verevon bør presenteres som **én arbeidsflate hvor mennesker og AI får arbeid gjort sammen**. Forsiden skal gjøre tre ting i denne rekkefølgen: gjøre kategorien forståelig, bevise én nyttig arbeidsflyt og åpne for å utforske bredden.

Den største muligheten er å knytte den visuelle kvaliteten til noe konkret. Vi har allerede mye av formspråket: typografien, luftigheten, de redaksjonelle kortene, bevegelsene og prefooter. Det som mangler, er en tydeligere sammenheng mellom motiv, budskap og faktisk produkt.

Mine viktigste vurderinger:

1. **Bytt heroens fjellmotiv**, men krev mer enn et menneske med laptop. Vi trenger synlig, troverdig arbeid og en komposisjon som passer den eksisterende teksten.
2. **Behold problemseksjonen.** Mer presise bilder kan gjøre dagens budskap vesentlig tydeligere uten en ny designrunde.
3. **La én faktisk oppgave bære produktseksjonen.** Flere scenarier blir fordypning. Å vise sju likeverdige demoer på forsiden vil gjenskape forklaringsproblemet.
4. **Gjør modulene til gjenkjennelige områder**, med korte beskrivelser og konkrete eksempler. Å bytte fra seks produktnavn til fem abstrakte begreper er ikke i seg selv en forbedring.
5. **Behold Senses foreløpig, med en egen menneskelig rolle.** Hvis den fortsatt gjentar produktseksjonen etter omskrivingen, bør den kortes ned. At den er vakker er en god grunn til å undersøke den videre, men ikke til å beholde overflødig forklaring.
6. **Utsett plattformens visuelle ombygging som ønsket.** Definer først hva vi faktisk kan dokumentere om lokasjon, kontroll, lagring og sertifisering.

### Det vi låser gjennom denne runden

- Dagens felles innholdsbredde: sentrert innhold inntil 1512 px, omtrent 100 px sidekant på bred desktop og eksisterende responsive innrykk. Bakgrunnsflater kan fortsatt gå til kanten.
- Hovedoverskriftenes etablerte prefooter-stil: Arbeit Light, vekt 300, dagens responsive størrelse, linjehøyde og bokstavavstand. Ingen ny generell nedskalering. Én semantisk H1 på siden; seksjonsoverskrifter kan ha samme visuelle stil uten å bli H1.
- Heroens grunnlayout, navigasjon, CTA-er og overgang til problemet.
- Problemseksjonens tekst, typografi, bildebevegelse, guidelinjer og etablerte høyde. Eksisterende minimumshøyde må fortsatt beskytte teksten på lave skjermer.
- De tre problemkortenes layout, kontrastforbedringer og etablerte høyde.
- Produktseksjonens innrykk, tydeligere sirkel og fravær av den tidligere femdelte kontrollstripen.
- Senses-animasjonene, modulkarusellens geometri og prefooter.

Dette er basert på dagens kildekode, lokal side, [forrige gjennomføringsnotat](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/2026-09-11-homepage-refinement.md>) og [strategibriefen](<C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/codex-file-preview-DThlqs/Verevon_Homepage_Strategy_Design_and_Implementation_Brief.md>). Den nye bestillingen om en menneskelig hero erstatter briefens tidligere føring om å beholde fjellmotivet. Eldre auditresultater brukes som historikk, ikke som bevis på dagens funksjonsstatus.

## 2. Hva referansene faktisk lærer oss

| Referanse | Observasjon | Relevant for Verevon |
| --- | --- | --- |
| [Gorgias](https://www.gorgias.com/) | Menneskelig situasjon med mobilbruk kobles til en konkret handle- og samtalekontekst i grensesnittet. | Koble mennesket til en oppgave. Et menneske alene forklarer ikke programvaren. |
| [Nolla](https://www.nollahealth.com/) | Den observerte heroen viser en menneskelig helsesituasjon, med et tydelig inngangspunkt til tjenesten. Knowledge-kortet gir en rolig kontrast til menneskebildene. | Vis hvem opplevelsen er til for. Bruk abstraksjon når den har en bestemt forklaringsrolle. |
| [Wonderful](https://www.wonderful.ai/) | Tydelig posisjonering rundt AI i virksomheter, med en konsentrert visuell presentasjon. Heroens film var pauset i nettleseren under gjennomgangen. | Ta med tydelighet og tilbakeholdenhet. Jeg bruker ikke denne inspeksjonen som belegg for en detaljert vurdering av filmens bevegelser. |

Jeg er derfor ikke helt enig i premisset om at alle disse lykkes fordi de viser mennesker som arbeider med teknologi. Nolla og Gorgias viser først og fremst en **relevant menneskelig situasjon**. Det er en mer nyttig regel for oss.

Dette støttes av brukbarhetsforskning: meningsbærende bilder kan hjelpe forståelsen, mens generiske stockbilder ofte får lite oppmerksomhet. Kvalitet og relevans må vurderes sammen. [NN/g: Memorable Imagery](https://www.nngroup.com/articles/7-tips-memorable-imagery/)

Samtidig er dette referanser og designobservasjoner, ikke innsyn i konkurrentenes konverteringstall. De beviser ikke at en bestemt hero, video eller fargepalett vil øke Verevons salg.

For bredde anbefaler jeg et enkelt første nivå og tydelige veier til detaljene. Det er anvendelsen av gradvis utforskning her: hovedsiden forklarer hva man kan oppnå; undersidene viser hvordan og hvor langt det går. [NN/g: Progressive Disclosure](https://www.nngroup.com/articles/progressive-disclosure/)

## 3. Én rolle per seksjon

| Seksjon | Spørsmålet den skal besvare | Endringen | Det den ikke trenger å gjenta |
| --- | --- | --- | --- |
| Hero | Hva er Verevon, og hvorfor er det relevant for oss? | Ny overskrift, ingress og menneskelig arbeidsmotiv. | Arkitektur og moduloversikt. |
| Problem | Hvorfor er dagens arbeid vanskelig? | Presise ekspert- og kunnskapsbilder. | Ny produktforklaring. |
| Tre kort | Hvilken friksjon kjenner jeg igjen? | Sammenhengende medieserie. | Alle funksjonene som løser problemet. |
| Produkt | Kan jeg faktisk delegere en oppgave her? | Én ekte oppgave fra forespørsel til et etterprøvbart resultat. | Hele produktkatalogen. |
| Senses | Hva blir bedre for menneskene som arbeider? | Ny tekst om sammenheng, faglig skjønn og retning. | Enda en gjennomgang av søk, chat og agenter. |
| Moduler | Hvilke deler av arbeidet kan dette støtte? | Fem forståelige områder med konkrete innganger. | Lange beskrivelser av hvert underverktøy. |
| Plattform, senere | Hvordan forvaltes data, tilgang og driftsansvar? | Dokumentert retning for suverenitet og tillit. | Agentens arbeidssteg en gang til. |
| Prefooter | Hvilken tanke skal sitte igjen? | Beholdes. | Nye argumenter. |

Arbeidshypotesen for hovedmålgruppen er norske virksomheter med kunnskaps- og kundeorienterte team, der arbeid går på tvers av kilder og systemer. Bredden i bildene skal vise kunnskapsarbeid; den skal ikke få forsiden til å fremstå som en ferdig spesialløsning for hver enkelt bransje.

## 4. Hero: konkret, menneskelig og rolig

### Forslag til hovedtekst

**Én arbeidsflate.**  
**For mennesker og AI.**

Verevon samler kunnskap, verktøy og oppgaver. Jobb selv, eller la Verevon ta neste steg — innenfor rammene dere setter.

Behold «Se arbeidsflyten» og «Be om tidlig tilgang». Den første skal lande ved starten på den konkrete demonstrasjonen, også ved direkte navigasjon til produktseksjonen.

Overskriften er tydeligere enn dagens «AI-plattformen for kunnskap og handling», men er heller ikke en unik posisjon alene. Særpreget må komme fra demonstrasjonen av at mennesker og AI faktisk kan arbeide i samme miljø, med forståelig tilgang og spor etter handlingene.

Et alternativ som kan testes mot hovedforslaget:

**Der mennesker og AI**  
**får arbeidet gjort.**

«Verevon. Der AI jobber.» fungerer godt som en kort merkevarelinje. Jeg ville ikke latt den bære hele heroen alene; en ny besøkende trenger også å forstå at dette er en arbeidsflate.

Formuleringen «Alt du kan gjøre manuelt, kan AI gjøre» beholdes som en produktambisjon. Som offentlig produktpåstand er den for absolutt før vi har kontrollert handlingene. Felles arbeidsflate betyr heller ikke at AI automatisk arver alle brukerens tilganger, eller må betjene hvert grensesnitt ved å klikke på skjermen.

### Brief for filmen

**Situasjon:** En person konsentrert om en konkret oppgave. Personen undersøker noe, sammenholder dokument og skjerm, gjør en vurdering eller arbeider med en kollega. Arbeidet skal være lesbart uten lyd.

**Komposisjon:** Motiv hovedsakelig til høyre, roligere felt bak teksten til venstre. Test med den faktiske overskriften før valg. Unngå ansikt og hender bak teksten. Egen mobilbeskjæring, ikke bare sentrert desktopvideo.

**Uttrykk:** Naturlig lys, varme nøytraler, tre, stoff og en dempet terrakottadetalj. Terrakotta er en forbindelse til merkevaren, ikke et krav om å farge alt oransje. Troverdige hudtoner prioriteres.

**Bevegelse:** Omtrent 8–12 sekunder som første redaksjonelle mål; ett godt opptak eller høyst et par rolige klipp. Ingen hektisk montage, kameraspinn eller skuespill mot kamera. Velg poster fra et godt, meningsbærende øyeblikk.

**Fravalg:** Håndtrykk, overdreven jubel, kaffe-og-laptop uten synlig oppgave, hologrammer, lesbare tredjepartsgrensesnitt som ser ut som Verevon, og nærbilder som skjuler hva personen gjør.

### Konkret søk: status på funnene

Disse er spor til videre utvelgelse, ikke ferdig godkjente heroer. Ingen av de undersøkte klippene er foreløpig valgt som vinner.

| Kilde og materiale | Status i gjennomgangen | Min vurdering |
| --- | --- | --- |
| [Pexels – cottonbro studio, mann med laptop på kontor](https://www.pexels.com/video/a-man-using-laptop-in-the-office-5483219/) | Forhåndsvisning visuelt inspisert. | Relevant arbeid, men kjølig grått kontoruttrykk. Ikke førstevalg for hero; mulig motiv i problemfeltet. |
| [Pixabay – OleksandrPidvalnyi, arbeid med laptop og headset](https://pixabay.com/videos/business-office-laptop-107425/) | Forhåndsvisning visuelt inspisert. Oppført som 1920 × 1080. | Tydelig aktivitet, men et mer hverdagslig og travelt bilde enn ønsket hero. Eventuelt støtteeksempel, ikke valgt. |
| [Pexels – KATRIN BOLOVTSOVA, kvinne med laptop](https://www.pexels.com/video/woman-using-a-laptop-5390854/) | Katalogside undersøkt; avspillingsbildet ble svart. | Kan undersøkes videre for naturlig lys. Utsnitt, format og uttrykk er ikke godkjent. |
| [Pexels – Mizuno K, arbeid med laptop](https://www.pexels.com/video/businessman-working-using-a-laptop-in-an-office-12896412/) | Katalogside undersøkt; avspillingsbildet ble svart. | Kandidat til videre kontroll, ikke grunnlag for et kvalitetsløfte. |
| [Unsplash – National Cancer Institute, forsker i laboratorium](https://unsplash.com/photos/laboratory-researcher-1fvqUP-xaYQ) | Foto og beskrivelse identifisert i katalogen. | Et konkret spor for problemseksjonens ekspertise. Må vurderes i den lille faktiske bildeflaten. |

Neste medieleveranse skal være **tre rangerte heroalternativer**, med avspilling, foreslått inn-/utpunkt, poster og desktop-/mobilbeskjæring lagt inn bak samme tekst. Søk først videre i sammenhengende serier fra aktuelle fotografer; det gir også bedre mulighet til å matche de tre problemkortene.

Bruk Pexels og Pixabay til film og Unsplash til foto/postere. Kontroller den enkelte ressursen; betalt Unsplash+-materiale må ikke forveksles med den åpne samlingen. Et advokatmotiv fra Getty/Unsplash+ dukket eksempelvis opp i søket og inngår ikke som et gratisvalg.

Et gratis klipp er bare et godt valg når det består motiv-, utsnitts- og kvalitetsvurderingen. Hvis stock ikke holder mål, er et kort, planlagt opptak med én person og ekte Verevon på skjermen en bedre reserve enn å akseptere et svakt hero-klipp. Eget opptak er en senere produksjonsbeslutning, ikke en utgift denne planen setter i gang.

## 5. Problemseksjonen: «Kunnskapen finnes» skal bli synlig

Behold overskriften «Kunnskapen finnes. Men den er spredt.» og ingressen:

> Verevon samler kildene, forstår sammenhengen og gjør neste steg klart — med dere i kontroll.

Behold også dagens CTA, lysbehandling bak teksten, bildeantall/plasseringer og bevegelsesparametere. Bytt innholdet i bildene uten å starte en ny runde med størrelse, hastighet eller seksjonshøyde.

Lag et utvalg på omtrent 12–16 unike bilder som kan fordeles i det eksisterende feltet:

| Motivgruppe | Visuell oppgave | Eksempel på hva vi ser etter |
| --- | --- | --- |
| Forskning og helse | Fagkunnskap i bruk. | Forsker med prøver, kliniker som sammenholder dokumentasjon. |
| Jus og rådgivning | Vurdering, tolkning og kilder. | Dokumentarbeid, faglig samtale, konsentrert gjennomlesning. |
| Teknikk og matematikk | Problemløsning. | Arbeidstegninger, utregning, fagperson ved et fysisk prosjekt. |
| Analyse og virksomhet | Oversikt og prioritering. | Analytikere ved et arbeidsbord, rapporter og notater i bruk. |
| Kunnskapskilder | Kunnskap finnes også utenfor én person. | Bøker, arkiv, diagrammer, dokumenter. |
| Samarbeid | Kunnskap er fordelt mellom mennesker. | To personer som gjennomgår samme sak. |

Velg store, tydelige former. Mange av partiklene er små; en komplisert rettssal eller finansskjerm vil bare bli støy. Jeg ville også valgt bort dommerhammer, børsneon og berømte personer som snarveier til «ekspertise».

Bland mennesker og kilder. Bare portretter gjør budskapet til «vi kjenner mange yrker», mens kombinasjonen viser hvor kunnskapen finnes. Bildene er illustrasjoner av kunnskapsarbeid, ikke dokumentasjon på kunder, anbefalinger eller sertifisert bruk i disse fagene.

**Ferdig når:** Utvalget oppleves som én redaksjonell samling, motivene er forståelige i faktisk størrelse, og tekst/motion/layout er bevart. Bildenes innhold vurderes både i bevegelse og i en statisk redusert variant.

## 6. Tre problemkort: én sammenhengende medieserie

Kortene har en god visuell struktur. Behold aktivt kort, navigasjon, høyde og tekstbehandling. Endre filmene som en samlet serie fremfor tre enkeltkjøp.

| Kort | Anbefalt scene | Meningen |
| --- | --- | --- |
| Det er ikke svaret som tar tid | En person leter i dokumenter, notater eller et arkiv. | Forarbeidet tar tid. |
| Ett spørsmål. Flere systemer. Ingen har hele bildet | En person sammenholder flere kilder, gjerne i samarbeid med en kollega. | Konteksten er fordelt. |
| Automatisering krever kontroll | En person gjennomgår et utkast eller tar en tydelig beslutning før neste steg. | Delegering trenger forståelige grenser. |

Jeg anbefaler å erstatte bil-/Cybercab-retningen. Den kan se imponerende ut, men fører tankene mot autonom kjøring og en annen merkevare. Menneskelig gjennomgang er nærmere det Verevon faktisk skal bevise. Dette er en anbefalt kursendring fra det tidligere medieønsket, ikke noe som er endret nå.

Match lysretning, metning, kontrast, kameraro og tempo. Fargegradering skal samle materialet, ikke skjule at klippene har helt ulik karakter. Bildene bør tåle å stå ved siden av hverandre uten tekst før de godkjennes. Konsistente bildeegenskaper bidrar til visuell sammenheng. [NN/g: Imagery in Visual Design](https://www.nngroup.com/articles/imagery-in-visual-design/)

**Anbefalt avspilling:** Gode stillbilder som utgangspunkt, med én kort forhåndsvisning om gangen ved et tydelig brukerinitiativ. Berøring og tastatur skal ha en tilsvarende tilgjengelig handling. Hover må ikke være eneste måte å forstå eller åpne et kort på. Lengre film og forklaring hører hjemme på undersiden.

Det er mitt svar på den tidligere diskusjonen om bilder versus video: stillbildet skal kunne gjøre hele kommunikasjonsjobben; video kan tilføre nærvær når brukeren vil se mer. Tre samtidige filmer er ingen dokumentert vei til bedre konvertering eller en designpris.

## 7. Produktseksjonen: fra stemning til bevis

Den dokumenterte sluttfilmen som allerede er laget, bruker en ekte SPA-composer med et **usendt eksempel**. Det er et nyttig produktutsnitt, men ikke et opptak av fullført arbeid. Neste nivå er en ekte, sammenhengende oppgave med et synlig resultat.

### Velg én hovedoppgave

**Første kandidat:** En uløst kundehenvendelse blir til et kildebasert svarutkast som et menneske kan gjennomgå.

Den kombinerer kunnskap, arbeid, AI og kontroll uten å kreve at besøkeren kjenner modulnavnene. Kundearbeid er et konkret eksempel, men innrammingen skal fortsatt være en bred arbeidsflate. Demonstrasjonen må ikke få hele produktet til å fremstå som bare en supportbot.

Prioriteringstabellen er en opptaksplan, ikke en påstand om at alle arbeidsflytene fungerer i dagens versjon. Hver rad trenger en faktisk gjennomføring før den blir markedsmateriell.

| Prioritet og plassering | Norsk oppgave til Verevon | Hva opptaket må kunne bevise |
| --- | --- | --- |
| P0 – hovedhistorie | «Finn grunnlaget for denne kundehenvendelsen og lag et svarutkast med kilder.» | Riktig henvendelse, reelle kilder, lesbart utkast og faktisk sted for gjennomgang/lagring. |
| P1 – utvidet innboksdemo | «Finn ukens uløste kundehenvendelser, prioriter de mest kritiske og klargjør svarutkast.» | Avgrenset liste, forklarbar prioritering og utkast knyttet til de riktige sakene. |
| P1 – agentfordypning | «Opprett en agent som gjør denne gjennomgangen hver morgen.» | Virkelig agentopprettelse, oppgave, verktøy, tidsplan, tilganger og hvor den kan stoppes. Hvis tidsplan ikke finnes, vis bare det som faktisk kan opprettes. |
| P2 – Social | «Lag forslag til neste ukes innlegg basert på kampanjebriefen, og legg dem klare for gjennomgang.» | Sammenheng med brief, kanaltilpasning og faktisk utkast. Planlegging/publisering vises bare hvis de stegene er koblet og virker. |
| P2 – Ads | «Hvilke kampanjer gikk svakere denne uken? Vis tallene og mulige forklaringer.» | Riktig sammenligningsperiode og underliggende data. Skill målte endringer fra hypoteser om årsak. |
| P2 – GitHub | «Undersøk denne feilen, finn relevant kode og klargjør et forslag til rettelse.» | Riktig repository, reell kodeendring og resultat av relevante tester. Et kodeforslag må ikke fremstilles som en testet eller publisert rettelse. |
| P2 – Insights | «Hva vet vi om spart tid i kundearbeidet denne måneden?» | Faktisk datagrunnlag og målemetode. Bruk antall gjennomførte oppgaver som alternativ hvis tidsbesparelse ikke kan dokumenteres. |
| P2 – datamaskin/support | «Koble til denne godkjente testmaskinen og undersøk problemet.» | Reell tilkobling, avgrenset sesjon, undersøkelse, funn og kontroll hos operatøren. En statisk fjernskjerm er ikke bevis på diagnose. |

### Sjekk produktet før manus låses

For den valgte oppgaven registrerer vi: inngang i UI, relevant handlingskontrakt, tilgang, tilgjengelig AI-verktøy, eventuell godkjenning, utført resultat og synlig spor. Samme eksempel skal kunne følges manuelt og gjennom naturlig språk der begge veiene faktisk er støttet.

Dette er den konkrete testen av idéen «Every interface is also an AI interface». Det er et sterkt produktprinsipp når handlingene deler kontrakter og kontroll. Det er et svakt markedsføringsløfte dersom vi viser grensesnitt som AI ennå ikke kan handle i.

Vi trenger ikke en ny full monorepo-audit for opptaket. Kontroller den valgte arbeidsflyten ende til ende og dokumenter hva den beviser. Tidligere auditfunn avgjør ikke alene hva dagens produkt kan gjøre.

### Manus for hovedopptaket

Første redaksjonelle mål er en film på omtrent 45–60 sekunder, med kortere utdrag i dagens produktseksjon. Tiden er et designmål, ikke et løfte om faktisk kjøretid.

1. **Oppgaven:** Vis én forståelig kundehenvendelse og hva som må avklares.
2. **Delegeringen:** Brukeren gir en kort, konkret instruksjon.
3. **Grunnlaget:** Verevon finner relevant kilde og viser hva utkastet bygger på.
4. **Arbeidet:** Et faktisk svarutkast dukker opp der arbeidet hører hjemme.
5. **Mennesket:** Vis reell gjennomgang, redigering eller godkjenning hvis flyten krever det.
6. **Resultatet:** Vis lagret utkast eller kvittering for en gjennomført handling. Avslutt ved det siste steget som virkelig har skjedd.

Velg et scenario som har et reelt kontrollpunkt. Ikke legg på en dekorativ godkjenningsmodal for å passe manus. Unngå også den generelle formuleringen «dere godkjenner før noe sendes, endres eller publiseres» hvis produktet tillater forhåndsgodkjente handlinger innenfor avtalte grenser.

### Slik brukes dagens design

- Behold den innrammede produktflaten og eksisterende seksjonsbevegelse.
- La de nåværende stadiene bære kapitler fra samme oppgave, fremfor fem løst beslektede stemningsbilder.
- Bruk abstrakt, dempet bakgrunn rundt et presist UI-utsnitt der det forbedrer lesbarheten. Produktet skal dominere.
- Bruk Remotion til innramming, utsnitt, pekere og kapitteloverganger. Svar, data og handlinger i produktet skal komme fra den dokumenterte gjennomføringen.
- Gi et tydelig valg for å se hele opptaket med pause og replay. Det skal ikke være nødvendig å scrolle gjennom flere skjermhøyder for å få med seg beviset.
- Flytt de øvrige scenarioene til relevante undersider og la modulene være inngangene.

Ta opp med demonstrasjonsdata i en kontrollert arbeidsflate. Behold originalopptaket sammen med den redigerte versjonen. Marker vesentlige tidskutt eller fremskynding, og kall det «opptak» når det er et opptak. Ikke legg inn oppdiktede fullføringsstatuser, prosenttall eller «live»-indikatorer.

**Ferdig når:** En ny besøkende kan forklare hva brukeren ba om, hva Verevon gjorde, hva det bygget på, og hva mennesket styrte. Det avgjør kvaliteten mer enn antall animasjoner.

## 8. Senses: mer rom for menneskelig arbeid

«Se Verevon i arbeid» konkurrerer med produktseksjonen. Senses bør i stedet handle om hva den felles arbeidsflaten gjør mulig for menneskene. Vi beholder det visuelle tempoet og animasjonene, men fjerner den gjentatte funksjonsforklaringen.

**Foreslått hovedoverskrift:**

**Mer rom for det**  
**mennesker gjør best.**

**Ingress:** Når kunnskapen er lettere å finne og oppgavene kan delegeres, blir det mer rom for å forstå, vurdere og velge retning.

| Kapittel | Ny tekstretning | Bildets rolle |
| --- | --- | --- |
| Se sammenhengen. | «Samle perspektivene rundt samme oppgave. La samtalen handle om hva dere ser og hva dere vil gjøre videre.» | Mennesker som faktisk undersøker noe sammen. Dagens samarbeidsmotiv kan passe. |
| Bruk fagkunnskapen. | «Et utkast er et utgangspunkt. Erfaringen deres avgjør hva som er relevant, hva som mangler og hva som holder.» | Konsentrasjon og vurdering. Dagens ansikt med koderefleksjon må vurderes kritisk; det kan signalisere AI mer enn faglig skjønn. |
| Velg retningen. | «Bestem hva som skal videre, hva som skal vente og hvor Verevon kan bidra. Deleger oppgaver med en tydelig hensikt.» | En person som setter retning i samarbeid med andre. Dagens delegeringsmotiv kan passe. |

Dette er tekstforslag til utprøving i eksisterende komposisjon. Vi skal ikke presse teksten inn i et bilde som sier noe annet. Hvis det andre motivet fortsatt oppleves som generisk teknologi, foreslås ett presist bildebytte uten redesign av seksjonen.

Meningsbærende bilder kan fungere i redaksjonelle, vekslende oppsett. Problemet oppstår når bildene bare pynter og gjør innholdet tyngre å skanne. Det støtter å forbedre innholdet i Senses før vi vurderer å erstatte layouten. [NN/g: Zigzag Page Layout](https://www.nngroup.com/articles/zigzag-page-layout/)

**Ferdig når:** Hvert kapittel tilfører en menneskelig verdi som ikke allerede er forklart i produktdemoen. Hvis et kapittel bare omskriver «søk, chat eller agent», skrives det om eller tas ut.

## 9. Moduler: behold kortene, gjør bredden forståelig

Dagens Knowledge, Research, Chat, Agents, Support og Trust gir en produktoversikt, men besøkeren må gjøre mye av sorteringsarbeidet selv. Jeg anbefaler å prøve **fem områder** innenfor samme karusell og kortgeometri. Dette er en organisering av forsiden, ikke et forslag til nye SKU-er, prismodeller eller endringer i SPA-navigasjonen.

**Foreslått seksjonsoverskrift:** «Én arbeidsflate. Flere måter å få gjort på.»

**Kort ingress:** «Fra kunnskapsgrunnlag til oppgaver, verktøy og oppfølging. Utforsk det som er relevant for dere.»

| Område | Forslag til korttekst | Konkret bredde i fordypningen | Visuell retning |
| --- | --- | --- | --- |
| Kunnskap | «Finn grunnlaget. Se sammenhengen.» / «Kilder, dokumenter og innsikt samlet rundt arbeidet dere skal gjøre.» | Søk, Research, kildehenvisninger og kontekst. | Egen kunnskapsstruktur inspirert av roen i Nolla-kortet. |
| AI og agenter | «Spør én gang. Deleger neste steg.» / «Fra en enkelt forespørsel til gjentakende oppgaver, innenfor avtalte rammer.» | Chat, agentopprettelse og faktisk støttede verktøy. | Ett lesbart oppdrag som blir til en konkret aktivitet. |
| Verktøy for arbeidet | «Arbeidet skjer her.» / «Samtaler, innhold og oppfølging på samme arbeidsflate.» | Eksempelvis Inbox, Social og Ads, i den grad de er tilgjengelige og dokumentert. | Ett gjenkjennelig arbeidsmiljø, ikke en mosaikk av miniatyrskjermer. |
| Datamaskiner og support | «Hjelpen kan gå videre.» / «Undersøk problemer og følg opp arbeidet der det skjer.» | Support og tilgjengelige datamaskin-/fjernsesjoner. | En faktisk avgrenset støttesesjon eller et menneskelig arbeidsmotiv. |
| Tilgang og kontroll | «Deleger med tydelige grenser.» / «Bestem tilganger, se hva som skjer og følg opp handlingene.» | Tillatelser, kontrollpunkter og dokumenterte aktivitetsspor. | En lesbar tillatelse eller gjennomgang fra produktet. |

AI/Agents og Knowledge er gode kategorier. «Verevon capabilities» er for internt og upresist som kundevendt kortnavn; «Verktøy for arbeidet» trenger konkrete eksempler for ikke å bli like vagt. «Datamaskiner og support» er også en hypotese vi bør teste: Hvis besøkende ikke forstår forholdet, må inngangen bli mer oppgaveorientert.

### Knowledge-kortet

Ta med prinsippet fra [Nollas Knowledge-kort](https://www.nollahealth.com/): lys flate, en tydelig kunnskapsvisualisering og lite tekst. Tilpass det til den eksisterende Verevon-geometrien. Ikke kopier den faktiske grafikken, knappene eller hele kortkomposisjonen.

Lag vår egen organiske struktur av tydelige kildegrupper. Noen få lesbare dokument-/kildesymboler kan forklare hva forbindelsene betyr. En langsom lokal respons ved interaksjon kan vise at en kilde inngår i en større sammenheng. Statisk tilstand skal være like god.

En tett, pulserende kule uten forklaring vil gjenta problemet fra den abstrakte heroen. Kunnskapsmotivet er vellykket når besøkeren leser «kilder i sammenheng», ikke bare «AI-partikler». Dersom illustrasjonen er konseptuell, skal den ikke se ut som et faktisk skjermopptak av en funksjon som ikke finnes.

### Tekst- og navigasjonsregler

- Behold dagens bildeproporsjon, typografiske hierarki, innrykk og karusellfølelse.
- Sikt mot én kort tittel og omtrent 15–25 ord forklaring per kort. Det er et redaksjonelt arbeidsmål, ikke en universell UX-grense.
- Flytt underfunksjoner til fordypningen. Unngå å kompensere for for mye innhold med flere små merkelapper eller mindre skrift.
- Prøv én tydelig tekstlenke per kort. Den skal beskrive hva man får se.
- Oppdater lenkemål når innholdet flyttes. Dagens kunnskapsanker leder til Senses; en omdøpt Senses-seksjon må ikke bli en tilfeldig destinasjon for Knowledge. Bruk en relevant eksisterende underside inntil en egen fordypning er klar.
- Ikke publiser et kort som lover en bredde dagens tilgjengelige produkt ikke støtter. Vis et smalere, sant eksempel inntil resten er dokumentert.

**Ferdig når:** En førstegangsbesøkende kan finne et relevant område uten å kjenne Verevons produktnavn, og hvert kort leder til noe som faktisk utdyper løftet.

## 10. Plattform: retning nå, ombygging senere

Plattformens nye rolle bør være å besvare fire praktiske spørsmål: **Hvem styrer tilgangen? Hvor behandles data? Hva beholdes? Hva kan virksomheten dokumentere?**

Det skiller den fra modulkortet «Tilgang og kontroll», som handler om styring av det daglige arbeidet. Plattformseksjonen handler om drift, dataforvaltning og virksomhetens krav.

Før vi tegner badges, lager vi et kort register med påstand, bevis, eier, avgrensning, dato og eventuell lenke til dokumentasjon:

| Tema | Hva vi må avklare før publisering |
| --- | --- |
| Suverenitet | Faktiske valg for drift, region, leverandører og administrativ kontroll. Regional lagring alene beviser ikke full suverenitet. |
| ZDR | Hvilke behandlingssteg og leverandører løftet omfatter. Skill behandling hos modellleverandør fra kunnskap, vedlegg og aktivitetsspor som brukeren ber produktet lagre. |
| GDPR | Konkrete forhold som databehandleravtale, behandlingsgrunnlag, sletting og underleverandører. Et generelt GDPR-symbol må ikke fremstå som en oppnådd sertifisering. |
| ISO | Standard, sertifisert juridisk enhet, omfang, utsteder og gyldighet. En leverandørs sertifisering er ikke automatisk Verevons. |

ISO utfører ikke selv sertifisering, og deres logo er ikke et fritt sertifiseringsmerke. GDPR-sertifisering har også en definert ordning; det er noe annet enn å sette «GDPR» i et skjold. [ISO: Certification](https://www.iso.org/certification.html), [ISO: Name and logo](https://www.iso.org/iso-name-and-logo.html), [EDPB: Certification](https://www.edpb.europa.eu/topics/accountability-and-compliance-tools/certification_en)

Anbefalingen er færre, dokumenterte utsagn med lenker fremfor en vegg av merker. Dagens plattformdesign beholdes i denne runden. Nye påstander og merker inngår først når grunnlaget er avklart.

## 11. Bevegelse: full opplevelse for deg, et ekte valg for andre

Koden tvinger nå full bevegelse ved å endre hva `window.matchMedia` rapporterer, samtidig som dokumentet markeres med full bevegelse. Det er en bred påvirkning av både egne komponenter og biblioteker. Den bør erstattes av en eksplisitt innstilling for Verevon-siden.

### Ønsket oppførsel

| Valg | Oppførsel |
| --- | --- |
| Følg enheten – standard | Respekter den besøkendes systemvalg. |
| Full bevegelse | Spill den komplette opplevelsen selv når enheten ber om redusert bevegelse. Lagres lokalt for denne nettleseren. |
| Redusert bevegelse | Vis alle budskap og resultater med rolig/statisk presentasjon. Lagres lokalt. |

Du skal kunne velge full bevegelse én gang og beholde dette valget. Nye besøkende skal ikke automatisk få ditt valg. Innstillingen kan ligge diskret i menyen og være lett å finne igjen. Den skal ikke kreve en oppstartsdialog.

### Gjennomføringen

1. Innfør én felles effektiv bevegelsesinnstilling som kombinerer lagret valg og systemvalg. Bruk den gjennom CSS, Motion, GSAP og videoavspilling.
2. Gi server og første klientrender en konsistent tilstand. Les nettleserpreferanser uten at første render får andre posisjoner, opacity-verdier eller innhold enn serveren. Ikke skjul avvik med `suppressHydrationWarning`.
3. Koble om dagens forbrukere før den globale overstyringen fjernes. Å slette overstyringen alene kan gjeninnføre feilen hvor produktseksjonen blir stående uten riktig animasjon eller innhold.
4. Sikre at bytte av innstilling rydder opp i scrollbindinger, tidslinjer og videostatus. Redusert visning skal ikke sitte igjen med store tomme scrollstrekninger eller usynlige stadier.
5. Hold dekorativ bevegelse og demonstrasjonsvideo atskilt. En bruker kan ønske full animasjon og samtidig pause en film.

En bakgrunnsfilm som starter automatisk og beveger seg i mer enn fem sekunder sammen med annet innhold, trenger som hovedregel en måte å pauses, stoppes eller skjules på. En bevegelsespreferanse alene erstatter ikke denne kontrollen. [W3C: Pause, Stop, Hide](https://www.w3.org/WAI/WCAG22/Understanding/pause-stop-hide.html)

### Verifikasjon som må inngå

- Ny besøkende med normalt systemvalg: forventet full opplevelse.
- Ny besøkende med redusert systemvalg: lesbart, fullstendig innhold uten tvungen stor bevegelse.
- Redusert systemvalg og lagret «Full bevegelse»: full opplevelse også etter oppfriskning og navigasjon.
- Valg endres mens produktseksjonen er synlig: ingen hengende pinning, dupliserte tidslinjer eller tomme paneler.
- Direkte inngang via `#produkt`, tilbakeknapp og mobil: riktig starttilstand og innhold.
- Ingen hydreringsfeil fra bevegelsesavhengige startverdier. Alle seksjoner kan brukes med tastatur.

Dette er en avgrenset teknisk oppgave som kan pågå mens heroens medier vurderes. Den er en forutsetning for å bedømme de nye opptakene riktig.

## 12. Medieproduksjon og ytelse

For hvert valgt medium lagres kilde-URL, fotograf, lisens, kontrollert dato, ønsket bruk, relevante rettighetsforhold, utsnitt, poster og endelig fil. Vi bruker ikke konkurrentenes filmer eller grafikk som egne produksjonsfiler.

Pexels og Pixabay tillater mye kommersiell bruk, men det er fortsatt vilkår rundt blant annet identifiserbare mennesker, merker og antydning om anbefaling. Unsplash-lisensen må kontrolleres mot ressursens faktiske samling. Dette sjekkes i utvelgelsen, ikke først når siden skal publiseres. [Pexels license](https://www.pexels.com/license/), [Pixabay license summary](https://pixabay.com/service/license-summary/), [Unsplash license](https://unsplash.com/license/)

Lever mediene i størrelser som passer bruken. Et lite bilde i problemfeltet skal ikke laste en original i full kameraoppløsning. Heroen får en prioritert poster og passende videoformat; filmer lenger ned lastes når de nærmer seg visning eller brukeren ber om dem. Videoer utenfor skjermen skal ikke fortsette unødvendig avspilling. [web.dev: Lazy-loading video](https://web.dev/articles/lazy-loading-video)

Sett et foreløpig arbeidsbudsjett på rundt 3 MB for første desktop-hero-loop og 1,5 MB for mobilvarianten. Dette er våre produksjonsmål, ikke en ekstern standard. Hvis nødvendig kvalitet krever mer, vurder kortere klipp, bedre komprimering eller poster med aktiv avspilling før budsjettet økes. Ikke last alle kortfilmer sammen med heroen.

Mål ytelsen i en produksjonsbygging, ikke bare gjennom Next.js-utviklingsserveren. De rapporterte kompileringstidene sier lite om en ferdig publisert brukeropplevelse. Målet er gode Core Web Vitals: LCP høyst 2,5 sekunder, INP høyst 200 ms og CLS høyst 0,1 ved 75-persentilen for reelle besøk. Før publisering brukes laboratoriemålinger som en indikator; de kan ikke erstatte feltdata. [web.dev: Web Vitals](https://web.dev/articles/vitals)

## 13. Gjennomføringsrekkefølge og leveranser

Vi begynner med heroen, som ønsket. Designen som allerede fungerer holdes stabil mens vi forbedrer historien og mediene. Ingen generell redesign av SPA, ny merkevare eller ombygging av infrastrukturen inngår.

| Fase | Arbeid | Konkret leveranse og ferdigkriterium | Avhengighet |
| --- | --- | --- | --- |
| 1. Hero først | Prøv de to norske tekstretningene. Fortsett målrettet filmsøk. | Tre vurderte medier og to helhetlige heroalternativer i eksisterende layout, med fungerende mobilutsnitt. Velg på forståelse og komposisjon. | Ingen ny produktopptak nødvendig. |
| 2. Bevegelsesvalg | Erstatt den globale tvangen med den lokale innstillingen. | Full/reduced/auto fungerer, ditt full-motion-valg huskes og produktseksjonen fungerer i alle relevante tilstander. | Kan utføres mens fase 1 vurderes. Må være stabil før endelig motion-kvalitetssjekk. |
| 3. Sammenhengende bilder | Sett sammen ekspertfeltet og de tre problemkortenes medieserie. | Kontaktark, utsnitt i faktisk størrelse, postere og klipp som henger sammen. Bevar seksjonenes etablerte design. | Heroens valgte lys/farge fungerer som referanse. |
| 4. Produktbevis | Gjennomfør P0-kandidaten og eventuelt en reserveoppgave. Lås manus etter resultatet. | Dokumentert kjøring, originalopptak, redigert hovedfilm og lesbare utdrag i produktseksjonen. | Krever testdata, fungerende workflow og kontrollert tilgang. Hvis et steg mangler, snevres historien inn. |
| 5. Senses og moduler | Prøv menneskelig Senses-tekst, fem områdeskort og egen Knowledge-visualisering. | Kortere tekster, én tydelig rolle per seksjon og relevante klikkmål. | Bygg på det den valgte demoen faktisk beviser. |
| 6. Helhet og publiseringsgrunnlag | Kontroller språk, overganger, tilgjengelighet, ytelse og førsteinntrykk. | Verifisert side med produksjonsmålinger og dokumentert forståelsestest. | Fase 1–5. |
| Senere. Plattform | Avklar påstander, dokumentasjon og ny formidling av infrastrukturen. | Egen godkjent innholdsretning før grafikk/badges lages. | Dokumenterte forhold, ikke bare ønsket posisjonering. |

Hver visuell beslutning gjøres på en konkret forhåndsvisning i siden. Vi unngår nye generelle prosentjusteringer av hele layouten som løsning på et lokalt problem. Det som er for tett, løses først med bedre prioritering, færre ord eller et riktigere utsnitt.

### Berørte deler ved senere implementasjon

| Del | Nåværende kilde |
| --- | --- |
| Hero | [HeroSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/HeroSection.tsx>) |
| Problembilder og tre kort | [ProblemSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProblemSection.tsx>), [ProblemCardsSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProblemCardsSection.tsx>) |
| Produktvisning | [ProductLoopSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopSection.tsx>), [ProductLoopProductDemos.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/ProductLoopProductDemos.tsx>) |
| Senses | [SensesSection.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/SensesSection.tsx>) |
| Moduler | [feature-workflow-cards.ts](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/feature-workflow-cards.ts>), [FeatureWorkflowCards.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/FeatureWorkflowCards.tsx>) |
| Bevegelsesvalg og felles uttrykk | [layout.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/layout.tsx>), [globals.css](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/globals.css>), [usePrefersReducedMotion.ts](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/shared/hooks/usePrefersReducedMotion.ts>) |
| Plattform, senere | [layer-section.tsx](<C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/home/sections/layer-section.tsx>) |

Kartlegg også alle forbrukere av bevegelsespreferansen før endring. Ta vare på opprinnelige medier og eksisterende lokale endringer. Oppdater sidetittel/metadata og relevante undersidetekster når den nye posisjoneringen innføres, slik at forsiden og søkeresultatet forteller samme historie.

## 14. Hvordan vi avgjør om det blir bedre

### Forståelse og forretningsmål

Det primære målet bør være kvalifisert interesse for tidlig tilgang eller en relevant samtale. Videoavspillinger, scrollengde og tid på siden kan forklare atferd, men er ikke mål på verdi alene.

Gjennomfør først en liten kvalitativ runde med omtrent 5–8 personer fra den antatte målgruppen. Dette er en praktisk første diagnose, ikke et statistisk bevis på konverteringsløft.

1. Vis heroen kort og spør hva de tror produktet er og hvem det er for. De skal ikke måtte gjette «AI-byrå», «konsulentselskap» eller «enda en chatbot».
2. La dem se hoveddemoen. Be dem forklare oppgaven, grunnlaget, resultatet og menneskets rolle.
3. Be dem finne et annet relevant bruksområde via modulkortene. Observer om navnene hjelper og om lenkemålet innfrir forventningen.
4. Spør hva Senses tilfører. Hvis svaret bare er en gjentakelse av produktdemoen, stram inn seksjonen.
5. Spør hva som fortsatt mangler for å ville prøve produktet. Det er viktigere enn om de synes fargen er fin.

Test tekst og film på en måte som gjør det mulig å forstå hva som hjelper. Ved sammenligning av to heroer bør man først holde teksten lik og variere filmen, eller omvendt. En full ombygging av alt samtidig gjør årsaken til eventuelle forbedringer uklar. Gjør en reell A/B-test først når trafikken er stor nok til et meningsfullt opplegg.

### Visuell kvalitet og Awwwards

Awwwards vekter design 40 %, brukbarhet 30 %, kreativitet 20 % og innhold 10 %. Det er vurderingskriterier, ikke en oppskrift som garanterer Site of the Day. [Awwwards: Evaluation](https://www.awwwards.com/about-evaluation/)

Min vurdering er at Verevon får mer særpreg fra en egen, velregissert produktfortelling og en gjennomarbeidet kunnskapsvisualisering enn fra flere stockvideoer. Stock kan etablere menneskelig nærvær; de mest særegne øyeblikkene bør komme fra vårt produkt og vårt visuelle arbeid. Det må fortsatt være lett å forstå og bruke siden.

### Samlet ferdigkriterium

- Heroen forklarer arbeidsflaten, og filmen viser relevant arbeid uten å konkurrere med teksten.
- Problemfeltets bilder forsterker dagens budskap; de tre kortene oppleves som én serie.
- Produktseksjonen viser et ekte resultat fra én oppgave. Ingen del av opptaket fremstiller planlagt funksjonalitet som fullført arbeid.
- Senses tilfører menneskelig verdi, og modulene åpner bredden uten å forklare hele produktet på nytt.
- Kunnskapskortet har en egen, forståelig visualisering og et relevant klikkmål.
- Plattformen lover ikke sertifiseringer, lagringsvilkår eller kontroll vi ikke kan dokumentere. Prefooter er bevart.
- Typografi og felles innholdskanter holder ved normal nettleserzoom, på bred desktop, laptop og mobil. Tekst blir ikke mindre for å få plass.
- Bevegelsesvalget fungerer uten hydreringsfeil eller fastlåste scrollseksjoner. Filmer kan pauses, og redusert visning inneholder hele historien.
- Produksjonsmålinger og faktiske forståelsestester er dokumentert. Ingen påstått konverteringsgevinst uten data.

## 15. Avklart nå og det som gjenstår å velge

**Planens anbefaling er klar:** hero først; samme visuelle system; menneskelig og relevant bildespråk; én faktisk produktoppgave; Senses om menneskene; færre og bredere modulinnganger; dokumentert tillit senere.

**Neste konkrete leveranse er hero-utvalget og tekst i faktisk layout.** Ingen ny global typografi- eller breddejustering inngår. Samtidig kan bevegelsesinnstillingen få sin avgrensede rettelse.

Det som ennå må avgjøres gjennom arbeidet, er hvilket filmklipp som faktisk holder kvaliteten, hvilken arbeidsflyt som fungerer best i dagens SPA, om alle fem modulnavn forstås, og hvilke infrastrukturpåstander som kan dokumenteres. Dette er tydelige beslutningspunkter med definerte leveranser, ikke grunner til å starte på nytt med hele siden.
