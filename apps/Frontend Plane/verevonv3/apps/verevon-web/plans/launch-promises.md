# Verevon: løfter som skal være sanne ved lansering

Status: lanseringskrav, **ikke** bekreftelse på at funksjonene finnes i dagens miljø. Denne listen knytter forsiden til produktbevis. Målbildet skal ikke snevres inn til det dagens demo kan gjøre; hvis et krav ikke er oppfylt, må det bygges og verifiseres før lansering.

## Posisjon

Verevon er arbeidsflaten som får arbeid gjort på tvers av bedriftens systemer. ERP, CRM, HRM og andre fagsystemer beholder sine data og sitt eierskap; Verevon kobler signaler, kunnskap, beslutninger og handlinger over dem. Chat er én inngang til dette arbeidet, ikke hele produktkategorien.

## Offentlige løfter og akseptansekriterier

| Løfte på nettstedet | Lanseringskrav som må demonstreres med virkelig tilkoblet data | Eiergrense |
| --- | --- | --- |
| «Én arbeidsflate» og «arbeidet på tvers» | En bruker kan åpne én sak fra et signal, se tilhørende kilder og oppgaver, stille spørsmål, følge utførelsen og se resultatet uten å miste sakens kontekst. Minst én ende-til-ende-flyt bruker flere avtalte systemer, ikke bare demo-fixtures. | Frontend/Application for arbeidsflaten; Ingestion for tilkoblinger; Data for kunnskap. |
| «Fanger opp det som må gjøres» | En faktisk hendelse kan opprette eller oppdatere en sak uten at brukeren først skriver en prompt. Duplikater, foreldede hendelser og manglende tilganger håndteres synlig og uten doble sideeffekter. Brukeren ser hvorfor Verevon foreslår akkurat denne oppgaven. | Ingestion for hendelsen; Model for tolkning; Application for sak og varsel. |
| «Bruker bedriftens kunnskap» og «kildene synlige» | En plan eller et svar viser hvilke dokumenter, meldinger og systemposter som støtter vesentlige påstander. Kildene kan åpnes av den autoriserte brukeren; manglende, motstridende eller foreldet grunnlag markeres. Bruker- og organisasjonstilgang gjelder også i søk og agentløp. | Control for identitet; Data for retrieval/spor; Model for syntese; Frontend for visning. |
| «Handler innenfor rammene dere setter» | Administrator kan definere tillatte verktøy, datatilganger, handlingsgrenser og hvem som må beslutte hva. En rutinesak kan fullføre flere godkjente steg uten klikk per steg. Forsøk utenfor rammene stoppes før sideeffekt, også ved retry, bakgrunnskjøring eller alternativ inngang. | Control for myndighet; Model for plan og agentløkke; Ingestion Integration for autorisert provider-utførelse; Quarry-v2 for nettleserhandlinger; Frontend for policy-UX. |
| «Dere tar beslutningene som betyr noe» | Avvik, usikkerhet og økonomisk eller irreversibel effekt løftes til riktig person **før** effektfulle handlinger. Vurderingskortet viser årsak, kilder, konsekvens og nøyaktig foreslått handling. Avslag, utløp og endring i tilgang hindrer utførelse; godkjenning gjelder bare den konkrete handlingen og kan ikke gjenbrukes. | Control for autoritet; Application Conversation for varig menneskelig intensjon; Model for sikker fortsettelse av agentløkken; Ingestion Integration for provider-effekt og engangskvittering; Quarry-v2 for nettleserutførelse. |
| «Kontroll hele veien» | Én sak har sammenhengende spor fra signal og kilder via plan, verktøykall og beslutning til resultat. Operatør kan se feil, stoppe videre arbeid og etterprøve hvem eller hva som gjorde hvert steg. Revisjonsdata følger avtalte regler for lagring og sletting. | Control for audit/retention; øvrige plan leverer korrelasjons-ID og hendelser. |

### Langsiktig relevans — også når AI er kostbart

Dette er en del av produktets lanseringsbar, ikke et nytt tallfestet spareløfte på forsiden. Hver arbeidsflyt skal ha synlig ressursbruk og en håndhevbar kostnadsgrense. Rutinearbeid må kunne bruke passende modell/verktøy fremfor alltid den dyreste modellen; et modellbytte skal ikke bryte kildespor, policy eller sakstilstand. Gevinster må måles mot fullført arbeid og menneskelig tidsbruk, ikke antall genererte svar.

## Scener som må bestå før publisering

1. **Bring / Plan:** En ekte leveringshendelse kobles med avtale og e-post. Verevon forklarer monteringskonsekvensen, foreslår oppfølging og stopper kun den utgående kundekommunikasjonen som krever beslutning. Kilder, sak og beslutning er sporbare.
2. **Fakturaavvik / Undersøk:** Verevon finner avviket mellom avtale, ordre og faktura, viser beregningen og klargjør et konkret neste steg. Ingen kreditnota eller annen økonomisk endring utføres uten riktig autorisasjon. Retry gir ikke dobbel effekt.
3. **Rutinearbeid / Delegering:** En autorisert rutineoppgave går fra inngang til resultat innenfor policy uten godkjenning av hvert mellomsteg. Brukeren kan se og stoppe arbeidet underveis.
4. **Høyrisiko / Oversikt:** En leverandøravtale løftes med grunnlag, konsekvens og riktig beslutningstaker. Et avslag eller utløpt mandat stopper videre handling, mens andre trygge saker kan fortsette.

For hver scene må teamet vise både lykkebane og minst én avvist eller degradert bane i et tilkoblet miljø. En film, poster, statisk mock eller fiktiv demo er kommunikasjon, ikke lanseringsbevis.

## Frigivelsesport

Lansering krever dokumentert ende-til-ende-test for scenene over, tilgangs- og tenant-isolasjon, håndtering av feil og avbrudd, samt en sikkerhetsgjennomgang av effektfulle handlinger. Ingen plan får «bestått» fordi bare frontend viser riktig tilstand; de ansvarlige planene må bekrefte kontrakt og faktisk utførelse. Dagens status og gjenstående arbeid må føres i eksisterende plane- og systemaudits, ikke antas fra denne listen.
