# Verevon-web — bilderetning og pin-kart

Arbeidsnotat fra bildegjennomgangen 2026-08-07. Pinterest-bildene er
**stemningsreferanser**, ikke lisensierte assets — de skal byttes mot egne
foto, AI-genererte bilder eller lisensiert stock før lansering. Filene ligger
i `public/verevon-mood/` slik at seksjonene kan vurderes visuelt nå.

---

## 1. Hva de fem referansesidene faktisk gjør med bilder

Gjennomgått direkte, ikke fra hukommelsen.

| Side | Bildebruk | Det vi tar med oss |
|---|---|---|
| **wonderful.ai** | Full-bleed *ekte* foto som hero (parabolantenne over bytak i skumring). Kortkarusellen «Solving the problems that move the business» er **fotokort**: hele kortet er et foto, med kort overskrift øverst til venstre og pil øverst til høyre. Ett foto = én historie (telemast på en ås, en person i kundemottak, en mørk teknisk render). | **Fotokort, ikke UI-mockup-kort.** Hvert kort får sitt eget, distinkte bilde. Bland lyst/mørkt, menneske/infrastruktur. |
| **cohere.com** | Ingen hero-bilde i det hele tatt — bare enorm type på hvitt. Kortene under er **foto som bakgrunn med ekte, mørk produkt-UI lagt oppå**. | Produkt-UI skal ligge *på* fotografi, ikke på pastellgradienter. Hero tåler å være ren typografi. |
| **intercom.com** | Ren, lys flate, produkt-UI i fokus, mennesker som støttebilder — aldri dekorativ abstraksjon. | Ikke bruk abstraksjon der et produktbilde er det ærlige svaret. |
| **stingray.no** | Ekstremt store luftrom. Venstre kolonne er nesten tom, med små versaler som ankere («PRESISJON», «OM STINGRAY»). Brødtekst i smal måleenhet til høyre. Ett stort dokumentarisk foto per idé — deres egen laser under vann. | **Ett bilde per idé, stort.** Mikro-etiketter i venstre marg. Fotografi skal være *spesifikt og ekte*, ikke generisk. |
| **terminal-industries.com** | Industriell dokumentarfotografi, teknisk overlay. | Overlay skal forklare bildet, ikke pynte det. |

**Fellesnevneren:** ingen av dem bruker pastell-3D-blobs. Alle fem bruker
**ekte fotografi**, stort, med ett motiv per idé. Det er nøyaktig gapet i
FeaturesSection og ProductLoopSection.

---

## 2. Diagnose av dagens bilder

### Feil stemning — rettet i denne omgangen

**`FeaturesSection.tsx`** — kortene kjørte på `FeatureCardFilms`:
- Bakgrunnene var lilla/rosa «celle»-mønster og pastellvasker — utenfor paletten.
- Mockup-UI-en i filmene sier fortsatt **«VELION»** og «Velion-kontekst» (kildekoden hadde allerede en TODO om dette på `FeatureCardFilms.tsx:38-43`).
- Seks kort delte bare **tre** unike filmer — `approve` gikk igjen tre ganger.

**`ProductLoopSection.tsx`**
- `human-haze.png` — lilla/oransje uskarpe figurer med fremmed tekst («Carmen», «F. Al-Sayed») brent inn i bildet. Utenfor paletten.
- `reflecting-with-coffee.jpg` — generisk stock-følelse, «brosjyre», ikke redaksjonelt.
- `soft-orb.png` — blå/rosa gradient-blob, utenfor paletten.

### Riktig stemning, men i overkant abstrakt — rettet

**`ProblemSection.tsx`** hadde to bildesystemer, og begge trakk mot abstraksjon:
- De tre store påstandskortene brukte metaforer (tåke, en fugl som letter, et tomt skrivebord). Leseren måtte dekode bildet før påstanden landet.
- Fuglen var **duplisert**: `/warm-flight.png` på kort 02 og `/27aab72a…jpg` i galaksen er samme fotografi.
- Galaksen var 8 stills, nesten utelukkende abstrakte renders.

Rettet (runde 1): de tre kortene viser nå det setningen faktisk handler om (en vegg av spredte vinduer, et menneske omgitt av systemene det skal forene, en person som står stille mens alt rundt beveger seg). Galaksen er tynnet fra 8 til 5 og beholder én myk render for luft.

**Runde 2 — tilbake mot sci-fi/natur/abstrakt.** Etter runde 1 ble kortene
vurdert som for literale igjen — for langt fra referansepunktet
`cohere.com`s andre seksjon (en mørk, glanset abstrakt render + et fotografi
side ved side). Byttet ut alle tre kort på nytt, denne gangen fra
`neon-velion`, `abstracts` og `scifi` (samt en ny, ubrukt tavle `natur`):
gullbrune lysstriper gjennom stormskyer (01 — lys som bryter gjennom for å
finne kilden), en hånd som trekker tråder til et strømerfelt av glitchete
skjermer (02 — «flere systemer, ingen har hele bildet», gjenbrukt fra runde 1
sin galakse), og et glassaktig, kontrollert flytende terreng i teal/rosa (03
— «automatisering krever kontroll»). Et opprinnelig funn — en nål i en
høyball fra `natur`, en god konseptmatch for kort 01 — ble forkastet: kildebildet
er bare 338×450px uansett hvilken størrelse-variant som ble hentet, for lite
for et kort som vises stort.

**Runde 3 — figurativt og narrativt, ikke ironisk.** Runde 2 traff
`cohere.com`s register, men gjorde kortene om til stemning som må dekodes —
en leser må slutte seg til at «glatt væske = kontroll» før påstanden lander.
Det er nettopp den typen indirekte metafor («ironi») som ikke skal brukes
her: hvert kort skal vise en gjenkjennelig figur som faktisk gjør det
setningen beskriver, med rom for ett stilisert eller surrealistisk innslag
—  ikke rendyrket abstraksjon, og ikke et symbol som krever tolkning.
- **Kort 01** gikk tilbake til `reading-at-desk-warm.jpg` (runde 1) — en
  person som faktisk sjekker en kilde og skriver notater. Ingen bedre
  figurativ kandidat er funnet ennå.
- **Kort 02** gikk tilbake til `swarm-of-screens.jpg` (runde 1) — ved nærmere
  ettersyn er dette allerede det mest presise bildet i settet: et hode som
  løses opp i hundrevis av spredte fotofragmenter som aldri utgjør ett bilde,
  bokstavelig «ingen har hele bildet», ikke et symbol for det.
- **Kort 03** er nytt: `undo-control-hand.jpg` — en hånd som trykker på
  flytende «Ctrl» og «Z»-taster, fra `scifi`. Snur/angrer er bokstavelig
  «tilbakerulling», som allerede er en av Agents' egne feature-chips —
  direkte kontrollhandling, ikke en stemning som skal stå for kontroll.

**Runde 4 — mer klassisk, sci-fi og premium.** Kort 01 og 03 holdt mål, men
kort 02s fotokollasj («hode løses opp i fragmenter») leste som griseterete
og amatørmessig sammenlignet med resten — riktig konsept, feil
produksjonsverdi. Byttet til `glitched-vision-city.jpg` (`scifi`): en mann i
skreddersydd svart jakke på en solfylt bygate, med et iriserende
glitch-visir som blokkerer øynene hans. Dette er strengere premium
(redaksjonell mote-/tech-kampanje-kvalitet), tydelig sci-fi (det digitale
visiret), og fortsatt direkte forklarende — synet hans er bokstavelig talt
blokkert, samme påstand («ingen har hele bildet») som forgjengeren, bare i
et mer polert register. Bildet hadde en synlig kredittekst («metronovon»)
nederst; kortets `imagePosition: "center 30%"` beskjærer den bort. Erstatter
`liquid-glass-terrain.jpg`.

**Runde 5 — samme kategori på tvers av alle tre.** Etter runde 4 var kort 02
sterkt («klassisk, sci-fi, premium»), men kort 01
(`reading-at-desk-warm.jpg`) var vanlig livsstilsfoto uten noe teknologisk
element, og kort 03 (`undo-control-hand.jpg`) var et klinisk sort-hvitt
studiobilde på hvit bakgrunn — tre forskjellige sjangre ved siden av
hverandre. Samlet dem i én kategori: et menneskelig kroppsdel gjennomskåret
av en digital/teknologisk effekt, samme varmnøytrale fargetone.
- **Kort 01** byttet til `code-on-face-profile.jpg` (`scifi`) — et ansikt i
  profil med kode reflektert over kinn og nese, blikket skarpt på teksten.
  Koden ER kilden og reglene («å finne kilden, sjekke reglene»), vist
  direkte på personen som leser den. (Merk: dette er en annen, mer polert
  variant av samme idé som Senses' `aruc-launcher-after.jpg` — ulike bilder,
  samme motiv-familie; ikke en duplikat, men verdt å være obs på siden
  begge ligger på samme side.)
- **Kort 02** uendret: `glitched-vision-city.jpg`.
- **Kort 03** beholdt konseptet (Ctrl+Z-hånden har ingen like direkte
  erstatning), men fikk en varm fargegradering (`undo-control-hand-warm.jpg`,
  ffmpeg `curves` + lett kontrast-/lysjustering) i stedet for nøytral
  sort-hvitt, slik at den hører til samme «serie» som 01/02 og ikke leser
  som et frittstående produktbilde.

**Runde 6 — direkte match mot selve overskriften, ikke brødteksten.** Founder
testet alle tre bildene mot brødteksten i detalj og var ikke fornøyd. Ba om
å fokusere på overskriftens mening spesifikt i stedet:
- Kort 02 («Ingen har hele bildet» — blokkert syn) og kort 03 («Automatisering
  krever kontroll» — Ctrl+Z er en reell, direkte kontrollhandling) holdt mål
  også mot den enklere overskrift-lesningen. Uendret.
- Kort 01 («Det er ikke svaret som tar tid») gjorde det ikke.
  `code-on-face-profile.jpg` handlet om *data/teknologi*, ikke om *tid* —
  det krevde min egen forklaring («koden er kilden») for å gi mening, som er
  akkurat den indirektheten runde 3 skulle fjerne. Founder svarte at ingen av
  de tre var gode nok og ba om et nytt forsøk.

Bygget et samlet, dedupert søk gjennom **alle** 211 unike bilder lastet ned
denne sesjonen (alle sju tavler), ikke bare de to-tre mest brukte, og lette
spesifikt etter det overskriften faktisk sier: **tid**. Fant
`hourglass-glass-render.jpg` (`neon-velion`) — et blankt, fargerikt glass-
timeglass i 3D-render. Dette sier «tid» på under et sekund, uten at leseren
må tenke seg fram til noe. Erstatter `code-on-face-profile.jpg`.

Bevisst avveining: timeglasset er en 3D-render, ikke et fotografi som 02/03,
så det bryter «samme kategori»-prinsippet fra runde 5 rent visuelt. Det ble
valgt likevel — å bli forstått umiddelbart veier tyngre enn at alle tre
bildene er tatt med «samme kamera».

**Runde 7 — tre fullt spesifiserte scener, kilde byttet fra Pinterest til
Unsplash.** Founder var fortsatt ikke fornøyd etter runde 6 og ga i stedet et
presist, ferdig-tenkt manus for alle tre kortene på én gang, pluss en
eksplisitt forbudsliste (nøytralt-blå «AI», flytende UI-grafikk, smilende
kontorfolk, generiske laptop-bilder, puslespillbrikker, tannhjul) og forslag
til søkesjangre («vintage control room photography», «switchboard operator
photography», «industrial human scale photography», osv.). Ingen av de sju
allerede nedlastede Pinterest-tavlene (kuratert rundt abstrakt/premium/sci-fi)
har industrielt eller byråkratisk motivmateriale av denne typen, så dette
søket gikk til Unsplash i stedet — reelle fotografier, ikke stemningsreferanser
som skal byttes ut senere. Unsplash-lisensen tillater kommersiell bruk uten
kreditering, så disse tre kan stå som de er ved lansering (i motsetning til
resten av `verevon-mood/`).

Alle tre er nå samme kilde-type (redaksjonelt fotografi) og bygger samme
serie founder beskrev — ett lite objekt i en stor prosess (01), én person
omgitt av mange usammenhengende kilder (02), én person som styrer en stor
automatisert mekanisme (03):
- **Kort 01** → `conveyor-belt-single-suitcase.jpg`: en ellers helt tom
  bagasjekarusell på en flyplass, med nøyaktig én koffert på beltet. Beltet
  er hele det synlige bildet; objektet som rir på det er lite — «tingen selv
  er enkel, reisen er det som tar tid», vist i stedet for påstått.
- **Kort 02** → `monitor-wall-lone-operator.jpg`: en operatør sett bakfra,
  foran en vegg av rundt 30 usammenhengende kamera- og systembilder (et
  kontrollrom for en modelljernbane). Ingen skjerm på veggen viser hele
  bildet — det er tretti delbilder, ikke ett.
- **Kort 03** → `aerospace-hand-pressure-gauge.jpg`: en hånd på en analog
  trykkmåler-vender, «LINE PRESSURE» synlig i bildet. En reell hånd på en
  reell kontroll, ikke en metafor for kontroll.

Erstatter `hourglass-glass-render.jpg`, `glitched-vision-city.jpg` og
`undo-control-hand-warm.jpg` — se §6 for orfaneringen.

**`PreFooterStatementSection.tsx`** hadde **ingen fotografier i det hele tatt** — de fem svevende «kortene» var rene CSS-gradienter med et par prikker og hårstreker hver. Samlet leste de som generisk UI-konfetti, og de gjorde dette til den andre heltabstrakte seksjonen på rad etter Problem. Rettet: fem ekte stills, samme posisjoner, samme GSAP-timeline (`data-prefooter-card` er beholdt).

### Gode bilder, tekst møter nå bildet — rettet

**`layer-section.tsx`** — bildet er bokstavelig talt fire plater stablet i dybden, men teksten var fire utbyttbare feature-blurber som aldri nevnte stabelen; du kunne stokke dem om uten at noe leste feil. Hvert lag har nå en posisjonsetikett (`Øverste lag` … `Nederste lag`) og en beskrivelse som sier hva det hviler på. Overskriften er byttet fra «Under hver arbeidsflate ligger samme kontroll.» til **«Fire lag. Ingen av dem skjuler de andre.»**

**`SensesSection.tsx`** — fotografiene var allerede riktige; teksten så bare ikke på dem. Hvert kapittel åpner nå på det menneskelige øyeblikket i sitt eget bilde: de tre kollegene som deler spørsmålet, ansiktet opplyst av det det leser, hånden midt i en forklaring.

Samme etterkontroll (RGB-snitt + faktisk motiv, ikke bare synsing) ble kjørt
på `layer-section` og `Senses` sine bilder. Senses' tre foto er ekte
fotografi (ett har til og med EXIF fra en Leica CL, kreditert fotograf —
lisensiert stock, ikke en Pinterest-skjermdump), og alle tre ligger i samme
varme register (rgb 92–180 på R, aldri kaldere enn nabofargen) — ingen
endring nødvendig der, bekreftet med data i stedet for antatt.

`layer-section` sine fire PNG-er er derimot **ikke fotografi** — de er egne
3D-isometriske UI-mockups. Der lå et helt annet problem:

### RETTET — ekte tredjeparts-varemerker i layer-section-mockupene

`infrastructure-layer.png` viste **OpenAIs faktiske logo** (seks-lappet knute
+ «AI»-tekst) og **Amazon Web Services' logo** (ordmerke + pil), pluss en
tredje mørk trekant-logo. `orchestration-layer.png` hadde samme OpenAI-knute,
umerket, midt i sentertilen. Dette er ikke en stemningsfeil — det er et reelt
varemerke-/troverdighetsproblem: å vise AWS på infrastruktur-laget motsier
direkte det som allerede er fastslått i dette prosjektet — Verevon kjører på
**Azure** og selges ikke som infrastruktur (samme grunn som «Verevon Cloud»
ble forkastet som modulnavn).

Rettet med `cv2.inpaint` (content-aware fill, ikke bare et fargeflekk-plaster)
for å fjerne merkene sømløst, deretter påført to nye generiske ikoner (en
«chip» og en «cloud», tegnet flatt og pikselnøyaktig perspektiv-vridd inn i
samme isometriske vinkel som platens øvrige ikoner — utledet fra platens egne
hjørnepunkter, ikke anslått) og ett nytt «hub»-ikon i orchestration-tilen.
Nye filer: `infrastructure-layer-v2.png`, `orchestration-layer-v2.png`. De
gamle filene ligger fortsatt i mappen (ureferert i kode) til de er godkjent
for sletting.

**Åpent, ikke rettet:** `business-context-layer.png` sine «Skill_2»-brikker
bruker blått («Knowledge», «Code») og rosa/rødt («MCP») i tillegg til den
merkevare-riktige oransje («Instructions») — det eneste stedet i hele
redesignet der blått og rosa opptrer sammen utenfor terrakotta-familien. Og
`management-layer.png` sitt innhold (en agent-samtalelogg + en
supportcase-trend) illustrerer et generisk support-dashbord, ikke «Policy,
godkjenning og revisjon» som laget nå hevder i tekst — et konseptuelt gap,
ikke et paletteproblem. Begge krever at eieren av mockup-kildefilen redigerer
selve UI-en, ikke et bildebytte.

### Fasit

`HeroSection.tsx` — solnedgangs-fjellandskapet med V-merket er allerede i
familie med wonderful.ai sin hero. Ikke rør den.

---

## 3. Modulrammeverket (forslag, ikke dokumentert produktsannhet)

Du foreslo «Verevon Chat / Knowledge / Agents / Support / Proof / Trust /
Cloud». Etter gjennomgang av dokumentene tok jeg med fem av dem og **droppet
én bevisst**:

- **Beholdt og reelt:** `Verevon Support` og Proof-sporet er dokumentert.
- **Nye navn på flater som faktisk finnes:** Knowledge, Research, Chat, Agents, Trust.
- **`Verevon Cloud` er utelatt.** Verevon kjører på Azure og selges ikke som
  infrastruktur. `verevon-feature-map.md` og trust-dokumentene advarer
  eksplisitt mot akkurat denne typen overclaim, og koden hadde allerede en
  kommentar som avviste å døpe om «Kilder» til «Cloud». Integrasjoner ligger
  nå som egenskaper under Knowledge og Agents i stedet.
- **Proof + Trust slått sammen** til `Verevon Trust` — de svarer på samme
  spørsmål (hva ble brukt, hvem godkjente, hva skjedde).

De seks er gruppert på løftet **Finn → Forstå → Få gjort**:

| # | Modul | Beat | Bilde |
|---|---|---|---|
| 01 | Verevon Knowledge | FINN | `module-knowledge.jpg` |
| 02 | Verevon Research | FINN | `module-research-v2.jpg` |
| 03 | Verevon Chat | FORSTÅ | `module-chat-v2.jpg` |
| 04 | Verevon Agents | FÅ GJORT | `module-agents.jpg` |
| 05 | Verevon Support | FÅ GJORT | `module-support-v2.jpg` |
| 06 | Verevon Trust | KONTROLL | `module-trust.jpg` |

---

## 4. Pin-kartet

Alle URL-er er verifisert ved nedlasting. `i.pinimg.com`-lenkene er
direktelenker til bildefila; pin-sidene er oppgitt der pin-ID-en ble fanget.

### 4a. Lastet ned og tatt i bruk nå

| Fil i `public/verevon-mood/` | Brukt i | Motiv | Tavle | Direktelenke |
|---|---|---|---|---|
| `module-knowledge.jpg` | Features, kort 01 | Blomst med datamålepunkter og grafnoder på svart | scifi | `https://i.pinimg.com/originals/20/8d/16/208d16c370bf56f9a8c46c6cc52c2af4.jpg` · [pin](https://no.pinterest.com/pin/469007748717853200/) |
| `module-research-v2.jpg` | Features, kort 02 | Kvinne ved laptop foran et vindu med gyllen høstløv utenfor | class | `https://i.pinimg.com/originals/12/83/4e/12834ed71c3c9c10d94531b36cd18221.jpg` |
| `module-chat-v2.jpg` | Features, kort 03 | To menn i samtale ved et bord, bylys i vinduet bak | class | `https://i.pinimg.com/originals/bb/cb/1f/bbcb1f1f9c9b4aeb56ff6db627ebff1c.jpg` |
| `module-agents.jpg` | Features, kort 04 | Person står stille mens arbeidet passerer i bevegelse | black-excellence | `https://i.pinimg.com/originals/46/97/ac/4697acec1bdd5476ddac7a8dba21faa3.jpg` |
| `module-support-v2.jpg` | Features, kort 05 | Kollega leser gjennom teamets skjermer i et strimlet, mørkt kontor | class | `https://i.pinimg.com/originals/39/4b/32/394b32d3313da7726203d870c45bbd60.jpg` |
| `module-trust.jpg` | Features, kort 06 | Hånd i terrakotta-erme som signerer | art | `https://i.pinimg.com/originals/c3/8f/ff/c38fffa580d94788fe512c3c4f0831e9.jpg` |
| `shared-surface-amber.jpg` | ProductLoop, steg 01 | To silhuetter holder samme opplyste ravgule flate | neon-velion | `https://i.pinimg.com/originals/06/49/71/0649715c3f8466ed0f6acb7dc0a100ac.jpg` · [pin](https://no.pinterest.com/pin/469007748717894609/) |
| `operator-calm-warm.jpg` | ProductLoop, steg 02 | Person i lys strikk med laptop, dempet rom | black-excellence | `https://i.pinimg.com/originals/ef/5c/80/ef5c80afc2227be0c78814893a1d3148.jpg` |
| `peach-wash.jpg` | ProductLoop, composer-bakgrunn | Ren fersken/oransje gradientvask | neon-velion | `https://i.pinimg.com/originals/6c/cf/45/6ccf4561d615ddaddacdcbc06ff70884.jpg` · [pin](https://no.pinterest.com/pin/469007748717929443/) |

### 4b. ProblemSection og PreFooter — i bruk nå

Ingen fil brukes i mer enn én seksjon. Det er en regel, ikke et sammentreff:
`desk-vast-white` og `window-city-dusk` lå først i begge, og ble skilt.

Kort 01–03 er fra og med runde 7 hentet fra **Unsplash**, ikke Pinterest — se
runde 7-notatet ovenfor. «Tavle»-kolonnen er derfor N/A og «Direktelenke»
peker til Unsplash-fotosiden, ikke en `i.pinimg.com`-fil.

| Fil i `public/verevon-mood/` | Brukt i | Motiv | Tavle | Direktelenke |
|---|---|---|---|---|
| `conveyor-belt-single-suitcase.jpg` | Problem, kort 01 | Én koffert alene på en ellers tom bagasjekarusell | Unsplash | `https://unsplash.com/photos/kj4e59Sf7Q0` |
| `monitor-wall-lone-operator.jpg` | Problem, kort 02 | Operatør bakfra mot ~30 usammenhengende kamera-/systemskjermer | Unsplash | `https://unsplash.com/photos/TtMKq3lJm-U` |
| `aerospace-hand-pressure-gauge.jpg` | Problem, kort 03 | Hånd på en analog trykkmåler-vender, «LINE PRESSURE» synlig | Unsplash | `https://unsplash.com/photos/H20OWckc-5E` |
| `desk-vast-white.jpg` | Problem, galakse | Person ved pult i et stort, tomt hvitt rom | black-excellence + art | `https://i.pinimg.com/originals/32/4d/51/324d51dffbdf1a5e98d0b2e5d1e759f8.jpg` |
| `window-city-dusk.jpg` | Problem, galakse | Silhuett ved vindu over by i skumring | black-excellence | `https://i.pinimg.com/originals/80/09/bf/8009bfd17482d4b51f5663abf1ca23d9.jpg` |
| `stair-descent-warm.jpg` | PreFooter, still 1 | Ensom figur i buet trearkitektur, varmt lys | art | `https://i.pinimg.com/originals/94/af/06/94af06f3df8b298d997eed1507cb0d69.jpg` |
| `hand-through-fabric.jpg` | PreFooter, still 2 | Hånd som presser gjennom hvitt stoff | scifi | `https://i.pinimg.com/originals/16/c5/00/16c500b934c6e541a2ca0de668cb807a.jpg` · [pin](https://no.pinterest.com/pin/469007748717853171/) |
| `hourglass-shadow.jpg` | PreFooter, still 3 | Timeglass med lang skygge, kremhvit | scifi | `https://i.pinimg.com/originals/6b/5a/35/6b5a35c19d6615f84df56f4cc1ca99b9.jpg` · [pin](https://no.pinterest.com/pin/469007748717853090/) |
| `seated-cream-terracotta.jpg` | PreFooter, still 4 | Person i lys dress mot brun vegg, uskarp mengde rundt | class | `https://i.pinimg.com/originals/25/81/3f/25813ff000d9243381da883c3808c74c.jpg` · [pin](https://no.pinterest.com/pin/469007748717879047/) |
| `peach-arch-walk.jpg` | PreFooter, still 5 | Person går gjennom fersken-farget bue | neon-velion | `https://i.pinimg.com/originals/f7/dd/ad/f7ddade6c5c26a6be2931974ece4018b.jpg` · [pin](https://no.pinterest.com/pin/469007748717864337/) |

**Ubrukt, fortsatt i mappa** — lastet ned som kandidater til `layer-section`, men den seksjonen ble til slutt en ren tekstjobb (bildene var allerede riktige). Behold eller slett:

| Fil | Motiv | Direktelenke |
|---|---|---|
| `caustics-room.jpg` | Rom med prismelys som faller i mønster på gulvet | `https://i.pinimg.com/originals/2f/f9/47/2ff947eeeac494a7ed17de4e394738bf.jpg` |
| `terracotta-pleats.jpg` | Terrakotta plissert bølgeform — lag på lag | `https://i.pinimg.com/originals/91/b3/1a/91b31a082d2342497657b9dd83b0aea0.jpg` |

### 4c. Sterke kandidater vi ikke lastet ned

| Motiv | Tavle | Passer til | Direktelenke |
|---|---|---|---|
| Mann bakfra mot en sverm av flytende skjermer, mørkt | scifi | ProblemSection — «kunnskapen er spredt» | `https://i.pinimg.com/originals/b7/7d/6e/b77d6ee02eaa701868f8c213a4ae1a12.jpg` |
| To hender med fin tekst viklet rundt | scifi | *allerede i bruk* som `hand-with-strings-of-text.jpg` | `https://i.pinimg.com/originals/08/26/da/0826da75d4feac9d41ce64754008cda7.jpg` |
| Hånd som presser gjennom hvitt stoff | scifi | Senses — «systemet kjenner deg» | `https://i.pinimg.com/originals/16/c5/00/16c500b934c6e541a2ca0de668cb807a.jpg` |
| Timeglass med lang skygge, kremhvit | scifi | PreFooter — tid/varighet | `https://i.pinimg.com/originals/6b/5a/35/6b5a35c19d6615f84df56f4cc1ca99b9.jpg` |
| Hånd med lysende datamesh i huden | scifi | Senses | `https://i.pinimg.com/originals/da/51/d9/da51d9fd946b5baccd957addeb55ade5.jpg` |
| Person i blå blazer som signerer papir | class | Trust (alternativ til `module-trust`) | `https://i.pinimg.com/originals/75/7f/53/757f5338d3c214d5ade7404632ada1b1.jpg` · [pin](https://no.pinterest.com/pin/469007748717894575/) |
| Team ved langbord med skjermer, ovenfra | class | Brand/kunde-seksjon | `https://i.pinimg.com/originals/4b/52/97/4b52970e2af0ca1299c21725c296b1e1.jpg` · [pin](https://no.pinterest.com/pin/469007748717894595/) |
| Mørkt møterom med oransje vindusglød | class | Support | `https://i.pinimg.com/originals/3e/9e/92/3e9e92cd5e594ddbf7bfd5c3b9b110d8.jpg` · [pin](https://no.pinterest.com/pin/469007748717894582/) |
| Person sett gjennom frostet glass | class | Trust — innsyn | `https://i.pinimg.com/originals/8d/2f/14/8d2f140ce9ad6df21edba26e28645b22.jpg` · [pin](https://no.pinterest.com/pin/469007748717894558/) |
| Ansikt gjennom reflekterende glass | class | Senses | `https://i.pinimg.com/originals/51/90/46/519046610a277b855e9917c4b0c2bcdc.jpg` · [pin](https://no.pinterest.com/pin/469007748717879051/) |
| Mann i oransje genser leser ved pult | black-excellence | Knowledge (alternativ) | `https://i.pinimg.com/originals/89/0d/7e/890d7e4cb5abf96a200fb5b28f8c083d.jpg` |
| Hender som holder telefon, hvit skjorte | black-excellence | Support — detaljbilde | `https://i.pinimg.com/originals/1a/bd/89/1abd89649c7c4615375079aea8333569.jpg` |
| Silhuett ved vindu over by i skumring | black-excellence | PreFooter | `https://i.pinimg.com/originals/80/09/bf/8009bfd17482d4b51f5663abf1ca23d9.jpg` |
| Tre personer over tegninger, varmt lys | art | Agents — samarbeid | `https://i.pinimg.com/originals/5e/aa/e1/5eaae1574fc879a97b1a8a45a3991cab.jpg` |
| Stol med lang skygge, grått | art | Trust — ro/objektstudie | `https://i.pinimg.com/originals/5f/87/62/5f8762b6b026a4bb24517b2b62310c6d.jpg` |
| Terrakotta dyne-bølge med ensom figur | neon-velion | Hero-variant / kapittelskille | `https://i.pinimg.com/originals/5f/e2/9b/5fe29ba4fd858acbb1a7732b701a413d.jpg` · [pin](https://no.pinterest.com/pin/469007748717929456/) |
| Monolitt-portal på vann, to figurer, solnedgang | neon-velion | PreFooter | `https://i.pinimg.com/originals/95/d9/2d/95d92db5a72896b4a49991b96855874d.jpg` |
| Person i hvitt strekker hånden mot fersken-kule | neon-velion + art | Senses | `https://i.pinimg.com/originals/2f/ca/c0/2fcac031f9ba7786083bd1e3fc4d73e4.jpg` · [pin](https://no.pinterest.com/pin/469007748717864335/) |

---

## 5. Regler vi bør holde oss til

1. **Ett bilde per idé, stort.** Ikke flere små dekorative bilder i samme blokk.
2. **Maks én abstrakt seksjon på rad.** Abstraksjon virker fordi den er
   sjelden. Problem og PreFooter ligger ved siden av hverandre i abstraksjons­grad
   — den ene bør bli konkret.
3. **Ingen tekst brent inn i bildet.** `human-haze.png` hadde fremmede navn i
   seg. Sjekk alltid hjørnene for vannmerker (`module-knowledge.jpg` har en
   liten signatur nederst til venstre som må vekk før lansering).
4. **Terrakotta/fersken/rav er varmefarger, ikke lilla.** Alt som drar mot
   lilla, magenta eller iriserende regnbue er utenfor paletten.
5. **Ekte mennesker foran abstrakte former** når seksjonen handler om arbeid.
   Abstraksjon er for overganger og løfter, ikke for produktforklaringer.
6. **Produkt-UI skal ligge på fotografi**, slik Cohere gjør — aldri på
   pastellgradient.

---

## 6. Åpne punkter

- `FeatureCardFilms.tsx` brukes fortsatt av `FeatureCardsSection.tsx` med de
  fire opprinnelige workflow-kortene. Filmene sier fremdeles «VELION» og må
  spilles inn på nytt eller pensjoneres.
- Bildene i `public/verevon-mood/` er Pinterest-referanser. De må erstattes
  før lansering. `module-knowledge.jpg` har i tillegg et synlig vannmerke
  («design/by/w1nzev») nederst til venstre.
- **RETTET** — kort 01 og 02 var begge mørke skjerm-motiver rett etter
  hverandre. Kort 01 er byttet til `reading-at-desk-warm.jpg` (en person som
  leser/skriver ved et pultfullt av papirer), som både matcher påstanden bedre
  — *kilder og regler*, ikke skjermer — og gir raden en varm/mørk/varm rytme
  i stedet for mørk/mørk/varm.
- **RETTET — modulkarusellens tre svakeste bilder.** En etterkontroll av alle
  seks modulbilder (gjennomsnittlig RGB per bilde, ikke bare synsing) fant at
  `module-research.jpg` var det ene bildet i settet med en tydelig kald
  blå-grå fargetone (rgb 126,133,140) mot resten sin varme
  amber/brun/terrakotta-familie. Samtidig var `module-chat.jpg` og
  `module-support.jpg` de to bildene der alt-teksten kun beskrev *hva som er i
  bildet* («et ansikt omsluttet av varme lysbaner», «portrett, lys skjorte»)
  i stedet for å knytte bildet til påstanden kortet gjør — i motsetning til de
  fire andre kortene, som alle har en alt-tekst som forklarer *hvorfor* bildet
  hører til der. Alle tre er byttet:
  - **Research** → `module-research-v2.jpg`: kvinne ved laptop foran et
    vindu med høstløv utenfor — bokstavelig «følg det som endrer seg utenfor
    huset», og varm i stedet for kald.
  - **Chat** → `module-chat-v2.jpg`: to menn i samtale ved et bord — en
    faktisk dialog, ikke bare et ansikt i lys.
  - **Support** → `module-support-v2.jpg`: en kollega som leser gjennom
    teamets skjermer før noe sendes — «et menneske som leser gjennom og
    sender», vist direkte i stedet for antydet.

  Alle tre kom fra `class`-tavlen, samme register som resten av kortene.
- **Foreldreløse etter omleggingen** — null referanser igjen i `src/`, verifisert
  med grep, trygge å slette: `svar-uten-kilder.webp`, `reflecting-with-coffee.jpg`,
  `DESIGN.jpg`, `0c49ef4d22d3f1ad1b49e59e374e1921.jpg`,
  `public/verevon-mood/module-research.jpg`, `module-chat.jpg`,
  `module-support.jpg` (de tre erstattede modulbildene),
  `calm-in-crowd-terracotta.jpg`, `light-through-clouds.jpg`,
  `liquid-glass-terrain.jpg`, `reading-at-desk-warm.jpg`,
  `swarm-of-screens.jpg`, `code-on-face-profile.jpg`, `undo-control-hand.jpg`
  (den opprinnelige sort-hvitt-versjonen, erstattet av
  `undo-control-hand-warm.jpg` i runde 5) — Problem-kortenes forkastede
  runder, se runde 2–6 ovenfor —
  `hourglass-glass-render.jpg`, `glitched-vision-city.jpg` og
  `undo-control-hand-warm.jpg` (runde 6-vinnerne, erstattet av tre
  Unsplash-foto i runde 7 — se runde 7-notatet) — og
  `public/verevon-layers/infrastructure-layer.png` +
  `orchestration-layer.png` (de to varemerke-versjonene — behold til
  erstatningene er godkjent, men de brukes ikke i kode lenger).
- **Fortsatt i bruk andre steder — ikke slett:** `warm-flight.png` (VisualPanel +
  `v3/ProblemSectionV3`), `human-haze.png` (FeatureCardsSection, `v3/ProductLoopSectionV3`,
  `extras/trust-scroll`), og galakse-stillsene `71fc9723…`, `b41e5460…`,
  `00631c87…`, `27aab72a…` (alle i `v3/ProblemSectionV3`). `v3/`-variantene
  er egne sider som ikke er del av denne omgangen — de har fortsatt de gamle
  bildene og den gamle stemningen.
