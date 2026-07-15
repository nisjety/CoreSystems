# Velion sub-pages — innhold, produktbevis og lanseringskrav

**Status:** Strategisk spesifikasjon for offentlig nettsted og produktflater.
**Grunnlag:** Konkurrentanalyse, nåværende Velion-hjemmeside og CoreSystem-dokumentasjon.
**Viktig dato:** Statusdokumentene med siste verifisering 13. juli 2026 overstyrer eldre, mer optimistiske produktbeskrivelser.

## Hensikt

Dette dokumentet skal styre hvilke undersider Velion trenger, hva de skal si, og hva som faktisk må fungere før en offentlig lansering. Det er ikke en plan for å fylle nettstedet med funksjonssider. Hver side skal bevise én konkret verdi som allerede er synlig i produktet.

Den sentrale fortellingen er:

> **Du trenger ikke å bytte ut verdenen din for å bruke Velion. Velion samler konteksten rundt arbeidet, foreslår neste steg og utfører bare det dere har gitt den lov til.**

Velion må ikke posisjoneres som «en helpdesk med AI» eller «en chatbot-bygger». Det riktige produktløftet er:

> **Kildebasert og kontrollert arbeid på tvers av systemene dere allerede bruker.**

Det betyr at siden må selge en arbeidsmåte:

1. Arbeid starter i et kjent system eller med et kjent signal.
2. Velion samler relevant kontekst og viser kildene.
3. Velion foreslår eller forbereder en handling.
4. Mennesket bestemmer når handlingen kan skje.
5. Resultatet, grunnlaget og neste endring er synlige i etterkant.

## Les dette før det skrives offentlig copy

### Lanseringsregelen

Velion skal ikke lansere en funksjonsside, logo, produktvideo eller påstand som ikke har et reelt, verifisert produktbevis bak seg. En attraktiv mockup kan forklare arbeidsflyten, men kan ikke stå inn for en ikke-utplassert sikkerhets-, personvern- eller kontrollmekanisme.

Dette er særlig viktig fordi de nåværende statusdokumentene beskriver flere harde produksjonsblokker: modellinferens og nødvendige gRPC-flater er ikke verifisert i drift, enkelte godkjennings- og tenantgrenser mangler live-bevis, og ende-til-ende ZDR/GDPR-bevis er ufullstendig. Se:

- [System production readiness](../../../../../SYSTEM_PRODUCTION_READINESS_2026-07-13.md)
- [Model Plane status](../../../../../apps/Model%20Plane/MODEL_PLANE_STATUS.md)
- [Data Plane v2 status](../../../../../apps/Data%20Plane%20v2/DATA_PLANE_STATUS.md)
- [Ingestion Plane status](../../../../../apps/Ingestion%20Plane/INGESTION_PLANE_STATUS.md)
- [GDPR summary](../../../../../apps/GDPR_SUMMARY.md)

### Kildehierarki for markedsføring

1. **Live, testet og tenant-sikkert i produksjon:** kan beskrives som tilgjengelig.
2. **Implementert i kildekode, men ikke utplassert eller live-verifisert:** kan ikke presenteres som lansert produktfunksjon.
3. **Design, roadmap eller konsept:** skal ikke vises som en vanlig funksjon eller ha salgs-CTA.

Historiske «live»-påstander i [VELION.md](../../../../../VELION.md) er eksplisitt ikke en nåværende lanseringssertifisering. Bruk dokumentet til produktretning og begreper, ikke som eneste bevis for offentlig copy.

### Hva Velion må eie språklig

| Ikke dette | Dette |
| --- | --- |
| «Erstatt alle systemene deres» | «Få mer ut av systemene dere allerede bruker» |
| «AI svarer automatisk» | «Velion forbereder, begrunner og handler innenfor rammene dere setter» |
| «Kunnskap i modellen» | «Kunnskap dere kan åpne, kontrollere og bruke» |
| «Trygg AI» som generisk merkeord | «Se kilde, tidspunkt, policy, godkjenning og historikk» |
| «Automatisering for automatiseringens skyld» | «Mindre leting, færre avbrudd og tydeligere ansvar» |

---

# 1. Hva hjemmesiden allerede kommuniserer

Den aktive hjemmesiden har allerede en god dramaturgi: **problem → arbeidsflyt/løsning → resultat → produktlag → avslutning**. Det er en sterkere start enn mange konkurrenter har, fordi den ikke begynner med en uforståelig AI-kategori.

## Nåværende seksjoner

| Del | Nåværende budskap | Hva den allerede gjør godt | Hva undersidene må utdype |
| --- | --- | --- | --- |
| Hero | «Fra signal til handling.» og «Kilder, svar og godkjenning i én arbeidsflyt.» | Setter Velion opp som et arbeidslag, ikke bare en chat. | Hva et signal er, hvilke systemer som kan gi kontekst, og hva kontroll betyr. |
| Koblinger | «Koblet til systemene deres» med Enhetsregisteret, Slack, Gmail, Notion, SharePoint, OneDrive, Outlook og Microsoft 365. | Reduserer bytteangst ved å vise kjente systemer. | Må bare vise dokumenterte koblinger eller tydelig merket tilgjengelighet. |
| Problemet | «Alle kan koble på en språkmodell. Færre kan vise kilden, la et menneske si ja — og likevel handle i tide.» | Gir Velion et tydeligere problem enn «AI er vanskelig». | Må lede videre til kontrollert arbeid og tillit, ikke en generell AI-side. |
| Funksjonskort | «Tenking, satt i arbeid» med Signal → Kontekst → Forslag → Godkjenning og kortene Bygg, Koble, Forstå, Deleger. | Er den tydeligste forklaringen på Velions operativmodell. | Hvert kort trenger en faktisk produktside med konkret flyt og bevis. |
| Senses | «Dere trenger ikke å bytte ut verden deres for å bruke Velion.» | Leverer resultatet av produktet: felles kontekst, forklaring og menneskelig kontroll. | Må bli hovedinngangen til Felles kontekst, Kontrollert arbeid og Tillit. |
| Produktlag | «Én flate for kunnskap, agentarbeid og kontroll.» | Forklarer at chat, innboks, søk og oppgaver henger sammen. | Trenger en informasjonarkitektur-side, ikke bare en estetisk lagillustrasjon. |
| Avslutning | «Hver kunde trenger riktig svar. Vi gjør svaret klart.» | Er konkret og menneskelig. | CTA må gå til en reell demonstrasjon, onboarding eller samtaleform, ikke en tom kontaktflate. |

### Kodeobservasjoner

- Den aktive hjemmesiden monterer Hero, koblingslisten, Problem, Feature Cards, Senses, Layer og avslutningen i [VelionHome.tsx](src/components/home/VelionHome.tsx).
- [ProductLoopSection.tsx](src/components/home/sections/ProductLoopSection.tsx) finnes med en god produktfortelling — Spør → Utkast → Godkjenn → Revider — men den er **ikke montert i den aktive VelionHome** nå.
- [TestimonialSection.tsx](src/components/home/sections/TestimonialSection.tsx) finnes også, men er ikke del av den aktive sidesekvensen.
- Hjemmesiden har allerede språk for kilde, policy, godkjenning, revisjon og tilbakerulling. Det er Velions mest differensierende språk og må ikke erstattes med vage ord som «smartere AI».

## Hjemmesidens viktigste styrker

1. Den viser en kontrollert arbeidsflyt i stedet for en isolert AI-chat.
2. Den bruker kjente systemer som overgang fra dagens arbeid til Velion.
3. Den lar Senses-seksjonen selge resultatet, ikke enda en funksjonsliste.
4. Den har en norsk og europeisk tillitsretning uten å lene seg på utelukkende compliance-språk.

## Hjemmesidens viktigste mangler før lansering

1. Det mangler tydelige destinasjoner for de store løftene: kontekst, kontroll, koblinger, kundearbeid og tillit.
2. Noen produktpåstander må knyttes til et tydelig live-bevis før de kan stå offentlig.
3. Den aktive siden må enten montere produktløkken eller fjerne/unngå CTA-er som lover en produktflyt uten en synlig destinasjon.
4. Koblingslogoer må reflektere faktisk, dokumentert støtte — ikke ønsket fremtidig støtte.
5. Tillitsløfter må være etterprøvbare. Ikke bruk «GDPR-compliant», «ZDR», «private» eller «godkjenning på alle handlinger» som absolutter før ende-til-ende-beviset finnes.

---

# 2. Felles mønster i konkurrentenes undersider

Konkurrentene selger forskjellige kategorier — kundeservice, enterprise search, CRM, people operations og AI-visibility — men de gjentar den samme grunnfortellingen:

> **AI skal ikke være enda et verktøy. Den skal være et kontrollert operativt lag inne i systemene bedriften allerede bruker.**

## Hva de gjentar

| Gjentatt budskap | Hvordan det selges | Hva Velion skal gjøre bedre |
| --- | --- | --- |
| Ingen smertefull migrering | «Koble til innboks, CRM, kalender eller dokumenter.» | Si tydelig at Velion blir en partner rundt dagens arbeid, ikke en ny silo. |
| Felles kontekst | Data fra samtaler, systemer, kilder og historikk brukes sammen. | Vis hvilken kontekst som brukes, ikke bare at «AI vet». |
| Handling, ikke bare svar | AI ruter, følger opp, søker, oppdaterer eller løser oppgaver. | Vis forslag, policy, godkjenning og reversering som én flyt. |
| Mennesket er fortsatt med | Operator, app, review, approval, permissions, guardrails. | Gjør menneskelig kontroll til produktets hovedbevis, ikke en sikkerhetsfotnote. |
| Kjente systemer reduserer frykt | Slack, Outlook, Notion, HubSpot, Shopify, Microsoft og Google brukes visuelt. | Bruk reelle logoer bare når koblingen har en ærlig status. |
| Tillit må være konkret | Kilde, logger, tilgang, rolle, deploy, sikkerhet og begrensning. | Forklar «hvorfor gjorde den dette?» med kildeutdrag, tidspunkt, policy, godkjenningsstatus og revisjonsspor. |

## Hvordan de ulike selskapene lærer oss noe

- **Wonderful** er best på enterprise-rammen: bygg, distribuer, overvåk og forbedre agenter i kontrollerte flater. Velion skal ta med kontroll, roller, spor og evaluering — men beholde et enklere, mer konkret arbeidsutgangspunkt.
- **Ayfie** er best på virksomhetskontekst og kildebevis: data blir i kildesystemene, og svaret er koblet til det som faktisk ble funnet. Velion skal gjøre dette operativt: kontekst skal kunne bli et forslag og en kontrollert handling.
- **Taito** er best på adopsjon gjennom hverdagen: teamet fortsetter i Slack, dokumenter og arbeidsflyter de kjenner. Velion skal adoptere dette språket: «arbeidet fortsetter, men med mindre leting».
- **Peec** er best på sekvens: signal → data → kilde → innsikt → handling. Velion skal bruke samme tydelighet for interne og kundevendte arbeidsflyter.
- **Attio** er best på kontekst som kontinuerlig oppdatert atferd: e-post, samtaler og data blir neste steg. Velion skal legge til sporbarhet og eksplisitt godkjenning.
- **Intercom og Fin** er best på tydelig kundearbeid: alle kanaler, én operatorflate, godkjenninger og pålitelige svar. Velion skal ikke prøve å late som om alt innen kanaler er ferdig før runtime er ferdig.

---

# 3. Foreslått informasjonsarkitektur

## Primærnavigasjon ved lansering

1. **Produkt**
   - Arbeidsflyten
   - Felles kontekst
   - Kontrollert arbeid
2. **Løsninger**
   - Kundearbeid
   - Overvåking og innsikt
3. **Integrasjoner**
4. **Tillit**
5. **Ressurser**
   - Dokumentasjon og produktnotater når det finnes reelt bevis

## Produktundersider som skal være reelle før de markedsføres

| Prioritet | Foreslått rute | Formål |
| --- | --- | --- |
| Kjerne | /produkt/arbeidsflyten | Vis hele løkken: kontekst → utkast → godkjenning → revidert handling. |
| Kjerne | /plattform/felles-kontekst | Vis at Velion samler historikk, kunnskap og neste steg uten å erstatte arbeidsverdenen. |
| Kjerne | /plattform/kontrollert-arbeid | Vis kilder, policy, risikonivå, godkjenning, revisjon og tilbakerulling. |
| Kjerne | /løsninger/kundearbeid | Vis innboks, oppfølging, kunnskapsgrunnlag og menneskelig overlevering. |
| Kjerne | /løsninger/overvåking-og-innsikt | Vis hvordan Velion oppdager endring, knytter den til kilder og foreslår neste steg. |
| Kjerne | /integrasjoner | Vis hva Velion kobles til, hva som synkroniseres, og nøyaktig tilgang/status. |
| Kjerne | /tillit | Vis dataflyt, tilgang, kildebevis, audit og reelle kontrollgrenser. |
| Når produktsurface er klar | /studio | Bygg og test agenter, regler, modeller og godkjenningsnivåer. |
| Når runtime og API-bevis er klart | /utviklere | Agent-/MCP-/API-integrasjon med tenant-sikker autentisering og dokumenterte grenser. |

## Hva som ikke bør være en lanseringsside ennå

- En bred «AI transformation»-side uten konkrete, verifiserte arbeidsflyter.
- En offentlig kanal-/voice-side før Channel Plane og de aktuelle kanalene er reelt klare.
- Bransjesider med løfter som krever spesifikke integrasjoner, compliance eller kundebevis som ikke finnes.
- En offentlig sertifiseringsside før sertifiseringene faktisk er gjennomført.
- En «ingen data forlater dere»-side før dette er en absolutt, testet og avgrenset produktgaranti.

---

# 4. Sidekapitler

## 4.1 /produkt/arbeidsflyten

### Jobben siden skal gjøre

Gjøre Velions modell forståelig på under et minutt:

> **Et signal blir til en kontrollert handling.**

Siden skal bygge videre på produktløkken, ikke gjenta Feature Cards. Feature Cards forklarer **hva Velion kan gjøre**. Denne siden viser **hvordan én oppgave faktisk beveger seg gjennom systemet**.

### Foreslått innhold

1. Hero: «Fra signal til kontrollert handling.»
2. Fire trinn med ekte produktbilder eller opptak:
   - Kontekst
   - Utkast
   - Godkjenning
   - Revider og forbedre
3. Et «se grunnlaget»-panel: kildeutdrag, tidspunkt, policytreff, usikkerhet og ansvarlig person.
4. Et «gjør det manuelt»-panel: samme handling skal kunne finnes i UI uten agent.
5. Et sluttbevis: handling, beslutning og endring kan inspiseres etterpå.

### Produkter som må fungere før siden er offentlig

- Chat eller arbeidsflate som faktisk kan starte en oppgave.
- Kilde- og kontekstbevis som er knyttet til svaret.
- Et reelt forslag med status.
- Varig, tenant-sikker godkjenning og beslutningshistorikk.
- Reviderings- og tilbakerullingsflyt for handlinger som påstås reversible.
- Et tydelig manuelt alternativ for samme oppgave.

### Copy-retning

- «Spør på vanlig språk. Velion samler det som er relevant før den foreslår noe.»
- «Se hvorfor forslaget kom, hva det bygger på og hva som skjer hvis dere godkjenner.»
- «Når noe må endres, endrer dere regelen eller kilden — ikke bare svaret denne gangen.»

### Relevante konkurrentreferanser, også overlappende

- [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Agent Studio](https://www.wonderful.ai/agent-studio), [Wonderful Apps](https://www.wonderful.ai/apps), [Wonderful AI Transformation](https://www.wonderful.ai/ai-transformation)
- [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Index](https://ayfie.com/ai-platform/index), [Ayfie Security](https://ayfie.com/ai-platform/security)
- [Taito Agents](https://taito.ai/agents), [Taito Performance](https://taito.ai/performance), [Taito Documents](https://taito.ai/documents)
- [Peec Agent Analytics](https://peec.ai/product/agent-analytics), [Peec AI Visibility](https://peec.ai/product/ai-visibility), [Peec Brand Perception](https://peec.ai/product/brand-perception), [Peec MCP](https://peec.ai/mcp)
- [Attio Workflows](https://attio.com/platform/workflows), [Attio Ask](https://attio.com/platform/ask), [Attio AI](https://attio.com/platform/ai), [Attio Data](https://attio.com/platform/data)
- [Fin Customer Agent](https://fin.ai/customer-agent), [Fin Operator](https://fin.ai/operator), [Fin Trust & Reliability](https://fin.ai/trust-reliability), [Fin Capabilities](https://fin.ai/capabilities)
- [Intercom Helpdesk](https://www.intercom.com/helpdesk), [Intercom Omnichannel](https://www.intercom.com/helpdesk/omnichannel)

---

## 4.2 /plattform/felles-kontekst

### Jobben siden skal gjøre

Gi brukeren følelsen av at Velion **ikke ber dem om å starte på nytt**.

Hovedbudskap:

> **Felles kontekst, før neste overlevering.**
> Velion samler historikken, kunnskapen og det som må skje videre.

Dette er ikke en «knowledge base»-side. Det er siden som forklarer hvorfor teamet slipper å lete i e-post, Slack, dokumenter, CRM og interne notater før de kan svare eller ta neste steg.

### Foreslått innhold

1. Hero med kjente arbeidsflater, ikke et abstrakt datasenter.
2. Ett konkret scenario: en kunde- eller intern sak går fra e-post/samtale til felles kontekst.
3. Vis kildekort og logoer for systemer som faktisk er koblet til.
4. Vis «hva Velion vet nå», «hva den fortsatt mangler» og «hva som bør skje videre».
5. Vis at mennesker kan åpne kilder, legge til kontekst og korrigere den.

### Produkter som må fungere før siden er offentlig

- Søk eller kontekstinnhenting med korrekte kildehenvisninger.
- Kildetilgang som respekterer organisasjon, bruker og tillatelse.
- Integrasjonsstatus, siste synk og frakobling som er sannferdig.
- Kunnskap som kan inspiseres, ikke bare en påstand om at «modellen vet».
- Robust håndtering av manglende, utdatert eller motstridende informasjon.

### Copy-retning

- «Historikken følger saken, også når den flytter mellom mennesker og systemer.»
- «Velion samler det dere allerede har — og gjør mangler synlige.»
- «Færre overleveringer som starter med: ‘Kan noen forklare hva som har skjedd?’»

### Relevante konkurrentreferanser, også overlappende

- [Ayfie Index](https://ayfie.com/ai-platform/index), [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Connectors](https://ayfie.com/ai-platform/connectors), [Ayfie Security](https://ayfie.com/ai-platform/security)
- [Attio Data](https://attio.com/platform/data), [Attio Ask](https://attio.com/platform/ask), [Attio Apps](https://attio.com/apps), [Attio Universal Context](https://attio.com/engineering/blog/introducing-universal-context)
- [Taito People Directory](https://taito.ai/people-directory), [Taito Performance](https://taito.ai/performance), [Taito Documents](https://taito.ai/documents), [Taito Operators](https://taito.ai/operators)
- [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Apps](https://www.wonderful.ai/apps), [Wonderful AI Transformation](https://www.wonderful.ai/ai-transformation)
- [Peec MCP](https://peec.ai/mcp), [Peec Agent Analytics](https://peec.ai/product/agent-analytics)
- [Fin Integrations](https://fin.ai/integrations), [Fin Customer Agent](https://fin.ai/customer-agent), [Fin AI Engine](https://fin.ai/ai-engine)
- [Intercom Omnichannel](https://www.intercom.com/helpdesk/omnichannel)

---

## 4.3 /plattform/kontrollert-arbeid

### Jobben siden skal gjøre

Bevise at Velion er et kontrollert arbeidslag, ikke en svart boks.

Hovedbudskap:

> **Velion foreslår. Dere bestemmer.**

Siden skal gjøre «hvorfor gjorde den dette?» til et enkelt produktløfte. Hvert reelt forslag bør kunne vise:

- kilde og kildeutdrag
- tidspunkt og ferskhet
- kontekst som ble brukt
- policy eller regel som ble matchet
- risiko- og godkjenningsstatus
- hvem som godkjente, reviderte eller stoppet handlingen
- audit- og eventuelt tilbakerullingsstatus

### Foreslått innhold

1. Hero: «Arbeid som kan forklares før det skjer.»
2. Ett beslutningskort med kilder, policytreff og mulige handlinger.
3. Tre tydelige valgmuligheter: Godkjenn, Revider, Gjør selv.
4. En etterpåvisning: hva som skjedde og hva som endret seg.
5. En egen del om grenser: hvilke handlinger Velion ikke kan gjøre uten tillatelse.

### Produkter som må fungere før siden er offentlig

- Rolle- og tenant-sikret godkjenningsflyt.
- Varig beslutnings- og auditlogg.
- Ekte policy-/risikostatus, ikke et visuelt eksempel som ser aktivt ut.
- Dokumentert tilbakerulling bare der den faktisk finnes.
- Feiltilstand som ikke later som om handlingen lykkes.
- Klart avgrenset modell for rettigheter, integrasjonsomfang og menneskelig ansvar.

### Copy-retning

- «Se grunnlaget før noe sendes, endres eller publiseres.»
- «Velion kan forberede arbeidet. Det kan ikke utvide sine egne rammer.»
- «Godkjenn når det er riktig. Revider når det trengs. Gjør jobben selv når du vil.»

### Relevante konkurrentreferanser, også overlappende

- [Wonderful Agent Studio](https://www.wonderful.ai/agent-studio), [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Apps](https://www.wonderful.ai/apps), [Wonderful Deployment](https://www.wonderful.ai/deployment)
- [Fin Trust & Reliability](https://fin.ai/trust-reliability), [Fin Operator](https://fin.ai/operator), [Fin API Platform](https://fin.ai/api-platform), [Fin Capabilities](https://fin.ai/capabilities)
- [Ayfie Security](https://ayfie.com/ai-platform/security), [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Index](https://ayfie.com/ai-platform/index)
- [Attio Workflows](https://attio.com/platform/workflows), [Attio Developers](https://attio.com/platform/developers), [Attio AI](https://attio.com/platform/ai), [Attio Data](https://attio.com/platform/data)
- [Taito Agents](https://taito.ai/agents), [Taito Documents](https://taito.ai/documents), [Taito Time Off & Attendance](https://taito.ai/time-off-attendance)
- [Peec MCP](https://peec.ai/mcp), [Peec Brand Perception](https://peec.ai/product/brand-perception), [Peec Agent Analytics](https://peec.ai/product/agent-analytics)
- [Intercom Helpdesk](https://www.intercom.com/helpdesk)

---

## 4.4 /løsninger/kundearbeid

### Jobben siden skal gjøre

Forklare hvordan Velion hjelper teamet med kundesamtaler, innboksarbeid, oppfølging og overleveringer — uten å kreve at leseren allerede kjenner ord som «helpdesk», «ticket» eller «chatbot».

Hovedbudskap:

> **Hver kunde trenger riktig svar. Velion gjør svaret klart.**

Siden bør selge resultatet: en kunde får et godt, riktig og begrunnet svar, og teamet slipper å lete etter status, policy og tidligere samtaler.

### Foreslått innhold

1. Hero med et kjent scenario: «Hvor er pakken min?» eller «Hva skjer med saken min?»
2. Vis en innkommende melding, kontekst fra systemer, forslag og menneskelig gjennomgang.
3. Vis hvordan saken kan gå videre til kollega, oppgave eller intern note.
4. Vis at det samme arbeidet kan utføres manuelt i innboksen.
5. Vis hva som skjer når Velion er usikker: den eskalerer eller ber om avklaring.

### Produkter som må fungere før siden er offentlig

- Ekte delte innbokser, samtaler, interne notater, status og tildeling.
- Kildebasert utkast eller en tydelig, reell «ikke nok grunnlag»-tilstand.
- Human-in-the-loop-kø for handlinger som krever vurdering.
- Korrekt kanalstatus: ikke markedsfør live chat, social, voice eller widget som tilgjengelig før kanalruntime finnes.
- Kundedata og historikk må være isolert og tilgangsstyrt.

### Copy-retning

- «Svar med historikken, policyen og neste steg samlet på ett sted.»
- «Når saken trenger et menneske, er overleveringen klar før den skjer.»
- «Velion gjør ikke kundearbeidet mindre menneskelig. Det fjerner letingen rundt det.»

### Relevante konkurrentreferanser, også overlappende

- [Intercom Helpdesk](https://www.intercom.com/helpdesk), [Intercom Omnichannel](https://www.intercom.com/helpdesk/omnichannel)
- [Fin](https://fin.ai), [Fin Customer Agent](https://fin.ai/customer-agent), [Fin Ecommerce](https://fin.ai/ecommerce), [Fin Sales](https://fin.ai/sales), [Fin Capabilities](https://fin.ai/capabilities)
- [Fin Channels](https://fin.ai/channels), [Fin Live Chat](https://fin.ai/channels/live-chat), [Fin Email](https://fin.ai/channels/email), [Fin Slack](https://fin.ai/channels/slack), [Fin Social Messaging](https://fin.ai/channels/social-messaging), [Fin Social Messaging anchor](https://fin.ai/channels#social-messaging), [Fin Voice](https://fin.ai/voice)
- [Wonderful Retail](https://www.wonderful.ai/industries/retail), [Wonderful Travel & Hospitality](https://www.wonderful.ai/industries/travel-hospitality), [Wonderful Financial Services](https://www.wonderful.ai/industries/financial-services), [Wonderful Media](https://www.wonderful.ai/industries/media)
- [Taito Operators](https://taito.ai/operators), [Taito Agents](https://taito.ai/agents)
- [Attio Call Intelligence](https://attio.com/platform/call-intelligence), [Attio Sequences](https://attio.com/platform/sequences), [Attio Ask](https://attio.com/platform/ask), [Attio Workflows](https://attio.com/platform/workflows)
- [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Connectors](https://ayfie.com/ai-platform/connectors)

---

## 4.5 /løsninger/overvåking-og-innsikt

### Jobben siden skal gjøre

Vise at Velion ikke bare reagerer på en samtale som allerede har kommet inn. Det kan følge med på kilder, endringer, markedssignaler og virksomhetsinformasjon — og levere et begrunnet neste steg.

Hovedbudskap:

> **Se endringen mens den fortsatt er mulig å handle på.**

Denne siden skal ikke love «full web intelligence» som et abstrakt supermenneske. Den skal vise en konkret flyt:

> Endring → kilder → vurdering → forslag → menneskelig beslutning.

### Foreslått innhold

1. En overvåket kilde eller virksomhetsendring.
2. Utdrag med tidspunkt og hvorfor endringen er relevant.
3. En kort innsikt eller handlingsanbefaling.
4. Valg: lagre, opprett oppgave, send utkast til godkjenning eller ignorer.
5. Tydelig ramme for lovlig innhenting, kildehensyn og brukerens ansvar.

### Produkter som må fungere før siden er offentlig

- Lovlig og policy-styrt web-/kildeinnhenting.
- Kildeproveniens, endringshistorikk og dato.
- Søk som viser hvor et svar kommer fra.
- Monitorering som ikke lover oppdagelse når jobben faktisk er forsinket eller avbrutt.
- Klar avgrensning av LinkedIn, konkurrentsporing og publiserte kilder basert på reelle tillatelser og vilkår.

### Copy-retning

- «Følg med på det som endrer seg rundt arbeidet deres — med kilden ved siden av.»
- «Velion peker på hva som har endret seg. Dere bestemmer om det skal bli til arbeid.»
- «Fra signal til neste steg, uten å miste grunnlaget på veien.»

### Relevante konkurrentreferanser, også overlappende

- [Peec Agent Analytics](https://peec.ai/product/agent-analytics), [Peec AI Visibility](https://peec.ai/product/ai-visibility), [Peec Brand Perception](https://peec.ai/product/brand-perception), [Peec Shopping](https://peec.ai/product/shopping), [Peec MCP](https://peec.ai/mcp)
- [Attio Reporting](https://attio.com/platform/reporting), [Attio Workflows](https://attio.com/platform/workflows), [Attio Data](https://attio.com/platform/data), [Attio Sequences](https://attio.com/platform/sequences), [Attio Ask](https://attio.com/platform/ask)
- [Taito Performance](https://taito.ai/performance), [Taito Founders](https://taito.ai/founders), [Taito Operators](https://taito.ai/operators)
- [Wonderful Blog](https://www.wonderful.ai/blog), [Wonderful AI Transformation](https://www.wonderful.ai/ai-transformation), [Wonderful Platform](https://www.wonderful.ai/platform)
- [Fin Research](https://fin.ai/research/), [Fin AI Engine](https://fin.ai/ai-engine), [Fin API Platform](https://fin.ai/api-platform)
- [Ayfie Index](https://ayfie.com/ai-platform/index), [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Connectors](https://ayfie.com/ai-platform/connectors)

---

## 4.6 /integrasjoner

### Jobben siden skal gjøre

Ta bort den største adopsjonsfrykten:

> **«Må vi bytte ut alt for å bruke dette?»**

Svaret skal være konkret:

> **Nei. Velion begynner med systemene dere allerede bruker, og dere ser hva den får lese eller gjøre i hvert enkelt tilfelle.**

### Foreslått innhold

1. En enkel logooversikt med kun reelle, tilgjengelige eller presist merket koblinger.
2. For hver kobling:
   - hva Velion kan hente
   - hva Velion kan foreslå
   - hva Velion eventuelt kan gjøre
   - hvilken tilgang som trengs
   - sist synkronisert
   - hvordan den kobles fra
3. Et scenario med Outlook/Slack/Notion/Microsoft 365 eller annen faktisk, dokumentert kobling.
4. En tydelig statusmodell: Tilgjengelig, beta med avgrensning, planlagt. Ikke bland dem.
5. Integrasjoner må knyttes til konkrete arbeidsresultater, ikke en logo-vegg.

### Produkter som må fungere før siden er offentlig

- Dokumentert integrasjon per logo.
- Riktig autorisasjon, tenantgrense og minst mulig tilgang.
- Sann status for synkronisering og feil.
- Frakobling, audit og datahåndtering per kobling.
- Ingen logo for Zendesk, Shopify, Meta, LinkedIn, Teams eller andre systemer med mindre produkt- og juridisk støtte er reell og påstanden kan verifiseres.

### Copy-retning

- «Velion møter dere der arbeidet allerede skjer.»
- «Koble til det som gir kontekst. Velg selv hva Velion får se og gjøre.»
- «Ingen ny silo. Ett tydeligere lag mellom systemene dere allerede bruker.»

### Relevante konkurrentreferanser, også overlappende

- [Ayfie Connectors](https://ayfie.com/ai-platform/connectors), [Ayfie Security](https://ayfie.com/ai-platform/security), [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Index](https://ayfie.com/ai-platform/index)
- [Attio Apps](https://attio.com/apps), [Attio Developers](https://attio.com/platform/developers), [Attio Data](https://attio.com/platform/data), [Attio AI](https://attio.com/platform/ai)
- [Taito Performance](https://taito.ai/performance), [Taito Agents](https://taito.ai/agents), [Taito People Directory](https://taito.ai/people-directory), [Taito Time Off & Attendance](https://taito.ai/time-off-attendance)
- [Fin Integrations](https://fin.ai/integrations), [Fin Channels](https://fin.ai/channels), [Fin Slack](https://fin.ai/channels/slack), [Fin Email](https://fin.ai/channels/email), [Fin Social Messaging](https://fin.ai/channels/social-messaging), [Fin Voice](https://fin.ai/voice)
- [Peec MCP](https://peec.ai/mcp), [Peec AI Visibility](https://peec.ai/product/ai-visibility)
- [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Apps](https://www.wonderful.ai/apps), [Wonderful Deployment](https://www.wonderful.ai/deployment)
- [Intercom Omnichannel](https://www.intercom.com/helpdesk/omnichannel)

---

## 4.7 /tillit

### Jobben siden skal gjøre

Gjøre tillit operativt, ikke dekorativt.

Hovedbudskap:

> **Dere skal kunne se hva Velion bruker, hva den får gjøre og hva som faktisk skjedde.**

Dette er ikke en juridisk tekstforkledning. Det er en produktflate med konkrete kontrollspørsmål:

- Hvilken data ble brukt?
- Hvilken kilde lå bak?
- Hvilken tilgang hadde Velion?
- Hvilken policy gjaldt?
- Hvem godkjente?
- Hva ble gjort?
- Hva kan angres eller revideres?

### Foreslått innhold

1. Trust Center-oversikt: apper, rettigheter, data brukt, «brukt av AI», retention og siste synk.
2. Et eksempel på en handling med kilde, policy, godkjenning og audit.
3. En ærlig side om datahåndtering, region, processor og begrensninger.
4. En side for tilgang/roller/SSO når dette er reelt klart.
5. En tydelig kontakt- og dokumentasjonsvei for enterprise-kunder.

### Produkter som må fungere før siden er offentlig

- Live og tenant-sikker audit for de handlingene som beskrives.
- Sammenhengende auth, roller og organisasjonsgrenser.
- ZDR, retention, erasure og «private»-påstander må ha ende-til-ende-bevis før de brukes som løfter.
- Ingen offentlig compliance-badge uten faktisk underlag, vurdering og juridisk godkjenning.
- Ingen påstand om full GDPR-overholdelse basert bare på en arkitekturintensjon.

### Copy-retning

- «Tillit er ikke en innstilling. Det er noe dere kan kontrollere i arbeidet.»
- «Åpne kilden. Se policyen. Se beslutningen.»
- «Velion skal ikke gjøre kontrollen usynlig når arbeidet blir raskere.»

### Relevante konkurrentreferanser, også overlappende

- [Ayfie Security](https://ayfie.com/ai-platform/security), [Ayfie Index](https://ayfie.com/ai-platform/index), [Ayfie Assistant](https://ayfie.com/ai-platform/assistant)
- [Fin Trust & Reliability](https://fin.ai/trust-reliability), [Fin Operator](https://fin.ai/operator), [Fin API Platform](https://fin.ai/api-platform), [Fin AI Engine](https://fin.ai/ai-engine)
- [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Deployment](https://www.wonderful.ai/deployment), [Wonderful Agent Studio](https://www.wonderful.ai/agent-studio), [Wonderful AI Transformation](https://www.wonderful.ai/ai-transformation)
- [Attio Developers](https://attio.com/platform/developers), [Attio Data](https://attio.com/platform/data), [Attio Universal Context](https://attio.com/engineering/blog/introducing-universal-context)
- [Taito Documents](https://taito.ai/documents), [Taito Agents](https://taito.ai/agents)
- [Peec MCP](https://peec.ai/mcp), [Peec Brand Perception](https://peec.ai/product/brand-perception)

---

## 4.8 /studio

### Jobben siden skal gjøre

Vise hvordan et team former Velion for sin måte å arbeide på — uten å late som om en tilfeldig prompt er produksjonsklar automatisering.

Hovedbudskap:

> **Bygg Velion rundt arbeidet deres, og test rammene før den får ansvar.**

Denne siden passer kortet **Bygg Velion**. Den skal samle agentoppsett, chatbot-oppsett, regler, tone, modellvalg, kilder, testeksempler og godkjenningsnivåer.

### Foreslått innhold

1. Beskriv jobben i vanlig språk.
2. Velg kilder, tilgang og systemer.
3. Definer regler, tone og når menneskelig godkjenning kreves.
4. Test på representative saker.
5. Se versjon, evaluering, endring og mulig tilbakerulling.

### Produkter som må fungere før siden er offentlig

- Agent-/regeloppsett som faktisk lagres og brukes i en runtime.
- Test- og evalueringsflyt med tydelig resultat og feiltilstand.
- Versjonering og tilbakeføring dersom siden lover dette.
- Policy og godkjenning som ikke bare er visuelle kontroller.
- Modeller og fine-tuning må bare nevnes dersom de er tilgjengelige, avgrenset og dokumentert.

### Copy-retning

- «Beskriv arbeidsoppgaven. Velg rammene. Test før du slipper den løs.»
- «En agent er ikke bare en prompt — den har kilder, tilgang, regler og grenser.»
- «Form Velion for dere, uten å miste innsikten i hva den gjør.»

### Relevante konkurrentreferanser, også overlappende

- [Wonderful Agent Studio](https://www.wonderful.ai/agent-studio), [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Apps](https://www.wonderful.ai/apps), [Wonderful Deployment](https://www.wonderful.ai/deployment)
- [Fin Operator](https://fin.ai/operator), [Fin Trust & Reliability](https://fin.ai/trust-reliability), [Fin AI Engine](https://fin.ai/ai-engine), [Fin API Platform](https://fin.ai/api-platform)
- [Taito Agents](https://taito.ai/agents), [Taito Founders](https://taito.ai/founders), [Taito Operators](https://taito.ai/operators)
- [Attio AI](https://attio.com/platform/ai), [Attio Workflows](https://attio.com/platform/workflows), [Attio Developers](https://attio.com/platform/developers)
- [Peec MCP](https://peec.ai/mcp), [Peec Agent Analytics](https://peec.ai/product/agent-analytics)
- [Ayfie Assistant](https://ayfie.com/ai-platform/assistant), [Ayfie Security](https://ayfie.com/ai-platform/security), [Ayfie Connectors](https://ayfie.com/ai-platform/connectors)

---

## 4.9 /utviklere

### Jobben siden skal gjøre

Gi tekniske kjøpere og integrasjonspartnere et sted å forstå hvordan Velion kan kobles til egne systemer — men bare når autorisasjon, tenantgrenser, audit og runtime faktisk er klar.

Hovedbudskap:

> **Koble Velion til arbeidet deres med de samme grensene som gjelder i produktet.**

### Foreslått innhold

1. Oversikt over sikre integrasjonsmønstre.
2. Autentisering og scopes.
3. Hva en agent kan lese, foreslå og utføre.
4. Hvordan godkjenning, audit og feilstatus vises.
5. API- og MCP-dokumentasjon med fungerende eksempler.

### Produkter som må fungere før siden er offentlig

- Dokumentert, autentisert og tenant-sikker API/MCP-flate.
- Riktig håndtering av OAuth, token, scopes og frakobling.
- Ikke publiser eksempler som lar agenten gå forbi policy eller menneskelig kontroll.
- Reelle negative tester for cross-tenant-tilgang og høy-risiko handlinger.
- Kall, feil, audit og retry må være konsistente nok til å støtte en utvikleropplevelse.

### Relevante konkurrentreferanser, også overlappende

- [Attio Developers](https://attio.com/platform/developers), [Attio Apps](https://attio.com/apps), [Attio Workflows](https://attio.com/platform/workflows), [Attio Data](https://attio.com/platform/data)
- [Peec MCP](https://peec.ai/mcp), [Peec Agent Analytics](https://peec.ai/product/agent-analytics)
- [Wonderful Platform](https://www.wonderful.ai/platform), [Wonderful Deployment](https://www.wonderful.ai/deployment), [Wonderful Agent Studio](https://www.wonderful.ai/agent-studio)
- [Fin API Platform](https://fin.ai/api-platform), [Fin Integrations](https://fin.ai/integrations), [Fin Trust & Reliability](https://fin.ai/trust-reliability)
- [Ayfie Connectors](https://ayfie.com/ai-platform/connectors), [Ayfie Security](https://ayfie.com/ai-platform/security)
- [Taito Agents](https://taito.ai/agents)

---

## 4.10 Bransje- og personasider

Wonderful, Fin og Taito viser at vertikale sider kan gjøre et komplekst produkt enkelt å forstå. Velion bør ikke starte med seks bransjesider som lover mer enn produktet beviser. Første fase bør være to modulære templates:

1. **Rolle:** kundeteam, operatør, leder eller founder.
2. **Arbeidsproblem:** kundearbeid, overlevering, overvåking eller kontrollert automatisering.

Når det finnes dokumentert produksjonsbevis, kan disse utvides med bransjesider.

### Side-template

1. Kjenn igjen arbeidshverdagen.
2. Vis ett før/etter-scenario.
3. Vis systemene som allerede brukes.
4. Vis kilder og kontroll.
5. Vis et målbart resultat eller et realistisk produktbevis.

### Ikke bruk denne retningen før beviset finnes

- Finans: ikke lov regulatorisk compliance uten juridisk og teknisk bevis.
- Helse: ikke antyd behandling av helsedata uten egnet databehandling og kontraktsgrunnlag.
- Telecom: ikke lov full kanal-/voice-automatisering uten reell plattformstøtte.
- Retail/e-handel: ikke vis ordrehandlinger eller refusjoner som automatiske før integrasjon, policy og godkjenning fungerer.

### Relevante konkurrentreferanser, også overlappende

- [Wonderful Media](https://www.wonderful.ai/industries/media), [Wonderful Healthcare](https://www.wonderful.ai/industries/healthcare), [Wonderful Telecommunication](https://www.wonderful.ai/industries/telecommunication), [Wonderful Retail](https://www.wonderful.ai/industries/retail), [Wonderful Travel & Hospitality](https://www.wonderful.ai/industries/travel-hospitality), [Wonderful Financial Services](https://www.wonderful.ai/industries/financial-services)
- [Fin Enterprise](https://fin.ai/solutions/enterprise), [Fin Ecommerce solution](https://fin.ai/solutions/ecommerce), [Fin Financial Services](https://fin.ai/solutions/financial-services), [Fin Ecommerce](https://fin.ai/ecommerce), [Fin Sales](https://fin.ai/sales)
- [Taito People Leaders](https://taito.ai/people-leaders), [Taito Operators](https://taito.ai/operators), [Taito Founders](https://taito.ai/founders), [Taito Performance](https://taito.ai/performance), [Taito Time Off & Attendance](https://taito.ai/time-off-attendance)
- [Intercom Helpdesk](https://www.intercom.com/helpdesk), [Intercom Omnichannel](https://www.intercom.com/helpdesk/omnichannel)
- [Peec Shopping](https://peec.ai/product/shopping), [Peec Brand Perception](https://peec.ai/product/brand-perception)

---

# 5. Hva Velion må bygge og bevise før offentlig lansering

Denne delen er en lanseringsport, ikke en roadmap-ønskeliste. Siden produktet skal lanseres først når alt er på plass, må alle aktive markedsføringsløfter ha et testbart og observerbart motstykke.

## 5.1 Kjernearbeidsflyten

- En bruker kan starte en oppgave i produktet.
- Velion kan hente riktig kontekst fra autoriserte kilder.
- Resultatet viser et klart skille mellom fakta, usikkerhet og forslag.
- Samme arbeid kan gjennomføres manuelt i UI.
- Høyere-risiko handlinger stopper ved en reell beslutning.
- Godkjenning, avslag, revisjon og utførelse kan inspiseres i ettertid.
- Feil, manglende data og utilgjengelige systemer vises ærlig.

## 5.2 Kunnskap og kildebevis

- Alle påstander om «kildebasert» må ha klikkbar eller tydelig inspeksjonsbar proveniens.
- Kildeutdrag, tidspunkt, synkstatus og eventuell usikkerhet må finnes når de vises i copy.
- Motstridende eller utdaterte kilder må ikke presenteres som sikker fasit.
- Tilgangsgrenser må holde på tvers av chat, søk, kunnskap, agentrun og integrasjoner.

## 5.3 Integrasjoner og adopsjon

- Hver publiserte logo har et teknisk og kommersielt sannferdig støtteomfang.
- Brukeren kan se rettighet, dataomfang, status og hvordan koblingen kobles fra.
- Ingen påstått kanalstøtte uten live kanalruntime.
- Ingen «private data»-påstand før per-bruker, organisasjon og indeks/retrieval-bevis er komplett.

## 5.4 Kontroll, audit og reversering

- Godkjenning må være tenant-sikker, varig og knyttet til den korrekte handlingen.
- Audit må kunne fortelle hvem, hva, når, med hvilken kilde/policy og resultat.
- Tilbakerulling kan bare vises der handlingstypen faktisk har en trygg reverseringsbane.
- Policygrenser må håndheves av runtime, ikke bare vises i UI.
- Retry og idempotens må hindre dobbeltutførelse ved feil og gjenopptak.

## 5.5 Personvern, sikkerhet og region

- Ikke bruk absolutt «GDPR compliant»-copy uten juridisk og teknisk releasebevis.
- ZDR må være testet på tvers av provider, cache, logg, sesjon, verktøykall, lagring og eksport før det omtales som ende-til-ende.
- Sletting, retention og data residency må ha testet operativ effekt, ikke bare metadata eller målarkitektur.
- Sensitive data, tokens, cookies og kundedata må ikke vises i markedsmateriale, demos eller testkontoer.
- Offentlig sikkerhetscopy skal beskrive dokumenterte mekanismer og eksplisitte grenser.

## 5.6 Produksjonsbevis

Før lansering kreves minst:

1. Utplassert og verifisert modellinferens, chat, embedding/retrieval og avhengige gRPC-kontrakter.
2. Positive og negative E2E-tester for auth, tenant-isolasjon, godkjenning og handling.
3. Inn- og utgående integrasjonsbevis med autentisering, avgrensning og feiltilstand.
4. Reell rollback-artifakt og øvd tilbakerulling for relevante tjenester.
5. Release- og sikkerhetsbevis fra Control, Data, Model, Ingestion og Application Plane.
6. Ingen uautentiserte eller cross-tenant-sensitive kontrollflater eksponert i drift.
7. Produkttekst, logoer, videoer og navngitte funksjoner kontrollert mot denne listen.

## 5.7 Interne dokumenter som skal styre denne porten

- [Velion product and reality record](../../../../../VELION.md)
- [CoreSystem codebase information system](../../../../../apps/CODEBASE_INFORMATION_SYSTEM.md)
- [System production readiness](../../../../../SYSTEM_PRODUCTION_READINESS_2026-07-13.md)
- [System status](../../../../../SYSTEM_STATUS.md)
- [GDPR summary](../../../../../apps/GDPR_SUMMARY.md)
- [Model Plane status](../../../../../apps/Model%20Plane/MODEL_PLANE_STATUS.md)
- [Model Plane verification](../../../../../apps/Model%20Plane/docs/VERIFICATION.md)
- [Data Plane v2 status](../../../../../apps/Data%20Plane%20v2/DATA_PLANE_STATUS.md)
- [Ingestion Plane status](../../../../../apps/Ingestion%20Plane/INGESTION_PLANE_STATUS.md)
- [Control Plane status](../../../../../apps/Control%20Plane/CONTROL_PLANE_STATUS.md)
- [Application Plane status](../../../../../apps/Application%20Plane/APPLICATION_PLANE_STATUS.md)
- [Quarry-v2 goal](../../../../../apps/Ingestion%20Plane/Quarry-v2/docs/GOAL.md)
- [Quarry-v2 policy](../../../../../apps/Ingestion%20Plane/Quarry-v2/docs/POLICY.md)
- [Quarry-v2 tenancy](../../../../../apps/Ingestion%20Plane/Quarry-v2/docs/TENANCY.md)
- [Model Plane graph memory](../../../../../apps/Model%20Plane/docs/phase-7/README.md)

---

# 6. Anbefalt rekkefølge for nettstedet

## Hjemmesiden skal gjøre

1. Få leseren til å kjenne igjen problemet.
2. Forklare Velions arbeidsflyt.
3. La dem kjenne resultatet: ro, oversikt og kontroll.
4. Sende dem til riktig bevis-side.

## Undersidene skal gjøre

| Hjemmesideseksjon | Naturlig destinasjon |
| --- | --- |
| «Fra signal til handling» | /produkt/arbeidsflyten |
| «Koblet til systemene deres» | /integrasjoner |
| Problemet med synlighet og menneskelig ja | /plattform/kontrollert-arbeid |
| «Bygg Velion» | /studio |
| «Koble arbeidet» | /integrasjoner og /plattform/felles-kontekst |
| «Forstå» | /plattform/felles-kontekst og /løsninger/overvåking-og-innsikt |
| «Deleger» | /plattform/kontrollert-arbeid |
| Senses kapittel 1 | /plattform/felles-kontekst |
| Senses kapittel 2 | /plattform/kontrollert-arbeid |
| Senses kapittel 3 | /tillit og /plattform/kontrollert-arbeid |
| Produktlag | /produkt/arbeidsflyten |

## Den beste gjenbrukbare CTA-en

Ikke: «Utforsk AI».

Bruk heller:

- «Se hvordan arbeidet beveger seg»
- «Se hva Velion bruker»
- «Se kontrollene»
- «Se systemene dere kan koble til»
- «Se hvordan dere former Velion»

---

# 7. Fullt register over konkurrent-URL-er

Lenker gjentas bevisst i sidekapitlene når de har overlappende relevans. Dette registeret sikrer at alle URL-ene fra analysen er bevart i én oversikt.

## Wonderful

- [Blogg](https://www.wonderful.ai/blog)
- [Media](https://www.wonderful.ai/industries/media)
- [Healthcare](https://www.wonderful.ai/industries/healthcare)
- [Telecommunication](https://www.wonderful.ai/industries/telecommunication)
- [Retail](https://www.wonderful.ai/industries/retail)
- [Travel & Hospitality](https://www.wonderful.ai/industries/travel-hospitality)
- [Financial Services](https://www.wonderful.ai/industries/financial-services)
- [Agent Studio](https://www.wonderful.ai/agent-studio)
- [Deployment](https://www.wonderful.ai/deployment)
- [Apps](https://www.wonderful.ai/apps)
- [AI Transformation](https://www.wonderful.ai/ai-transformation)
- [Platform](https://www.wonderful.ai/platform)

## Ayfie

- [Connectors](https://ayfie.com/ai-platform/connectors)
- [Security](https://ayfie.com/ai-platform/security)
- [Index](https://ayfie.com/ai-platform/index)
- [Assistant](https://ayfie.com/ai-platform/assistant)

## Taito

- [People Leaders](https://taito.ai/people-leaders)
- [Operators](https://taito.ai/operators)
- [Founders](https://taito.ai/founders)
- [Performance](https://taito.ai/performance)
- [Time Off & Attendance](https://taito.ai/time-off-attendance)
- [Agents](https://taito.ai/agents)
- [Documents](https://taito.ai/documents)
- [People Directory](https://taito.ai/people-directory)

## Peec

- [MCP](https://peec.ai/mcp)
- [Agent Analytics](https://peec.ai/product/agent-analytics)
- [Shopping](https://peec.ai/product/shopping)
- [Brand Perception](https://peec.ai/product/brand-perception)
- [AI Visibility](https://peec.ai/product/ai-visibility)

## Attio

- [Apps](https://attio.com/apps)
- [Reporting](https://attio.com/platform/reporting)
- [Sequences](https://attio.com/platform/sequences)
- [Introducing Universal Context](https://attio.com/engineering/blog/introducing-universal-context)
- [AI](https://attio.com/platform/ai)
- [Developers](https://attio.com/platform/developers)
- [Call Intelligence](https://attio.com/platform/call-intelligence)
- [Workflows](https://attio.com/platform/workflows)
- [Data](https://attio.com/platform/data)
- [Ask](https://attio.com/platform/ask)

## Intercom

- [Helpdesk](https://www.intercom.com/helpdesk)
- [Omnichannel](https://www.intercom.com/helpdesk/omnichannel)

## Fin

- [Fin home](https://fin.ai)
- [Customer Agent](https://fin.ai/customer-agent)
- [Trust & Reliability](https://fin.ai/trust-reliability)
- [Research](https://fin.ai/research/)
- [API Platform](https://fin.ai/api-platform)
- [AI Engine](https://fin.ai/ai-engine)
- [Integrations](https://fin.ai/integrations)
- [Enterprise](https://fin.ai/solutions/enterprise)
- [Ecommerce solution](https://fin.ai/solutions/ecommerce)
- [Financial Services](https://fin.ai/solutions/financial-services)
- [Social Messaging](https://fin.ai/channels/social-messaging)
- [Slack](https://fin.ai/channels/slack)
- [Social Messaging anchor](https://fin.ai/channels#social-messaging)
- [Live Chat](https://fin.ai/channels/live-chat)
- [Email](https://fin.ai/channels/email)
- [Voice](https://fin.ai/voice)
- [Channels](https://fin.ai/channels)
- [Operator](https://fin.ai/operator)
- [Ecommerce](https://fin.ai/ecommerce)
- [Sales](https://fin.ai/sales)
- [Capabilities](https://fin.ai/capabilities)

## Bredere konkurransegrunnlag fra den tidligere posisjoneringsgjennomgangen

Disse sidene var del av den bredere referanserammen for Velion, men ikke av den detaljerte undersiderevisjonen over. De er beholdt for sammenligning når nye undersider skrives:

- [Mimir](https://trymimir.com/)
- [Gorgias](https://www.gorgias.com/)
- [Chatbase](https://www.chatbase.co/)
- [Zendesk](https://www.zendesk.com/)
- [ElevenLabs](https://elevenlabs.io/)
- [Linear](https://linear.app/)
- [Taito](https://taito.ai/)
- [Intercom](https://www.intercom.com/)
- [Ayfie](https://ayfie.com/)
- [Wonderful](https://www.wonderful.ai/)
- [Attio](https://attio.com/)
- [Zero](https://zero.inc/)
- [Peec](https://peec.ai/)

---

# 8. Kort beslutning

Velion har allerede den riktige overordnede fortellingen på hjemmesiden:

> **Problem:** Det holder ikke at AI svarer; arbeidet må kunne forstås og kontrolleres.
> **Løsning:** Velion samler kontekst, lager forslag og arbeider innenfor tydelige rammer.
> **Resultat:** Teamet bytter ikke ut verdenen sin — de får mindre leting, færre avbrudd og mer ro i arbeidet.

De viktigste undersidene å ferdigstille er derfor:

1. Arbeidsflyten
2. Felles kontekst
3. Kontrollert arbeid
4. Kundearbeid
5. Overvåking og innsikt
6. Integrasjoner
7. Tillit

Studio og utviklersiden skal følge når den tilhørende produktsurface har reelt runtime-, sikkerhets- og driftbevis. Lanseringen skal ikke måles på antall sider eller animasjoner, men på om hvert løfte kan inspiseres i produktet.
