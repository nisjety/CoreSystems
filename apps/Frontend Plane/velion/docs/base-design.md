# Base Design System

---

## 0. Hva Velion er

Velion er en AI-helpdesk og kunnskapsplattform for team som vil la en agent
svare kunder og ansatte med selskapets egen kunnskap.

Produktet kobler seg til kildene organisasjonen allerede bruker:

- nettside og offentlig firmadata
- Slack, Microsoft 365, Notion, Google Drive og andre arbeidsverktøy
- dokumenter, tickets, e-post, chat og kunnskapsbaser

Velion bygger dette om til en levende kunnskapsbase, viser relasjonene i et
grafisk kunnskapskart, og lar teamet lage AI-agenter som kan svare, søke,
rute, eskalere og rapportere uten at teamet må sette opp tung infrastruktur.

Velion skal derfor føles som:

- et rolig kontrollrom for support og kunnskap
- en presis agentbygger, ikke en leken chatbot-demo
- et trygt system for bedrifters interne og eksterne data
- et produkt som gjør små team i stand til å operere som større supportteam

Designet må støtte denne posisjonen. Det skal være stille, skarpt og
tillitsbyggende, med nok varme til at AI-agenten føles hjelpsom og menneskelig.

---

## 1. Design Philosophy

Dette designet er bygget på:

- Tynne linjer  
- Matte flater  
- Skarpe kanter  
- Minimal border  
- Enkle og ærlige bilder  
- Mye rom og luft  

Alt skal føles:

- Kontrollert  
- Stille  
- Presist  
- Intellektuelt  
- Tidløst  

Designet skal ikke imponere med volum.  
Det skal imponere med disiplin.

---

# 2. Visual Identity

# Fargesystem

Designet bygger på varme, matte og kontrollerte toner.  
Ingen harde gradienter. Ingen glossy effekter.  
Bakgrunn skal føles fysisk – nesten som papir eller kalk.

Velion sin palett har tre nivåer:

- **Brand**: varme papirflater, presis charcoal og coral som retningslys.
- **Product shell**: lys Linear-inspirert dashboard, sidebar og navbar.
- **System states**: små, tydelige aksenter for info, suksess, advarsel og fare.

---

## Brand Palette

| Token | HEX | Bruk |
|---|---:|---|
| `velion.ink` | `#111111` | Primary buttons, definitive controls, active states |
| `velion.ink-soft` | `#1C1C1C` | Large titles, active sidebar text, modal headings |
| `velion.charcoal` | `#2B2B2B` | Primary body/title text on warm surfaces |
| `velion.text` | `#3A3C44` | Dashboard/sidebar navigation text |
| `velion.text-muted` | `#66615B` | Auth/onboarding descriptions |
| `velion.text-subtle` | `#8A8D96` | Helper text, muted controls |
| `velion.paper` | `#F4F1EB` | Warm page background |
| `velion.canvas` | `#EDEBE7` | Auth/onboarding modal shell |
| `velion.surface` | `#EAE6DF` | Warm cards, secondary panels |
| `velion.panel` | `#FFFFFF` | Inputs, product cards, dropdown panels |
| `velion.border` | `#D8D2C6` | Warm borders, shortcut pills |
| `velion.border-strong` | `#C8C1B3` | Stronger warm dividers |
| `velion.coral` | `#FF2E63` | Signature accent, active indicator, recommendation ring |
| `velion.coral-hover` | `#FF4D7A` | CTA hover, micro feedback |
| `velion.coral-glow` | `#FF3B5C` | Scanner line, animation glow only |
| `velion.coral-deep` | `#8B1E3F` | Dark blend/shadow for coral visuals |
| `velion.success` | `#10B981` | Verified, secure, connected states |

---

## Dashboard, Sidebar og Navbar

| Token | HEX | Bruk |
|---|---:|---|
| `velion.dashboard-sidebar` | `#F7F7F8` | Sidebar and navbar background |
| `velion.dashboard-main` | `#FCFCFD` | Main dashboard canvas |
| `velion.dashboard-panel` | `#FFFFFF` | Dashboard panels and cards |
| `velion.dashboard-border` | `#E6E6E8` | Product shell border |
| `velion.dashboard-accent` | `#5E6AD2` | Linear-style secondary accent |
| `velion.dashboard-active` | `#F0F1F5` | Active nav row |
| `velion.dashboard-hover` | `#F6F7F9` | Hover nav row |
| `velion.dashboard-divider` | `#ECECF1` | Sidebar separators |
| `velion.dashboard-input-border` | `#E2E3E9` | Search/input border |
| `velion.sidebar-rail` | `#111318` | Dark icon tile/rail contrast |
| `velion.sidebar-icon` | `#6B6E78` | Default sidebar icons |
| `velion.sidebar-muted` | `#9B9EA8` | Muted sidebar icons/chevrons |
| `velion.sidebar-placeholder` | `#B0B3BC` | Placeholder and disabled text |
| `velion.navbar-divider` | `#E4E0D8` | Navbar vertical divider |
| `velion.navbar-search-text` | `#5F5A52` | Search trigger text |
| `velion.navbar-search-muted` | `#989286` | Search icon and muted warm labels |
| `velion.navbar-pill-text` | `#615B52` | Recent-search pill text |
| `velion.dark-sidebar` | `#191A1D` | Dark/minimized sidebar background |
| `velion.dark-main` | `#1F2023` | Dark main product surface |
| `velion.dark-panel` | `#202124` | Dark panels |
| `velion.dark-border` | `#2B2D31` | Dark borders |

---

## System States

| Token | HEX | Bruk |
|---|---:|---|
| `velion.info` | `#3578F6` | Unread badges, informational emphasis |
| `velion.info-bg` | `#E8F1FF` | Info badge background |
| `velion.warning` | `#DD7A1F` | Notifications, focus ring, attention marker |
| `velion.warning-strong` | `#B96618` | Warning text/icons on pale amber |
| `velion.warning-bg` | `#FFF1DE` | Warning badge background |
| `velion.danger` | `#C0402A` | Destructive actions |
| `velion.tooltip` | `#1A1A1A` | Tooltip surface |

---

## Base

### Warm Off-White (Primary Background)

Føles som kalk / papir.

HEX:
#F4F1EB

Bruk:
- Hovedbakgrunn
- Store flater
- Layout foundation


### Warm Surface (Cards / Panels)

Litt mørkere, men fortsatt myk.

HEX:
#EAE6DF

Bruk:
- Kort
- Paneler
- Input-bakgrunn


### Beige (Subtle Separation)

Arkitektonisk, varm separasjon.

HEX:
#D8D2C6

Dypere variant:
#C8C1B3

Bruk:
- Tynne borders
- Subtile linjer
- Sekundære flater


### Primary Text (Charcoal)

Ikke ren svart. Mykere.

HEX:
#2B2B2B

Bruk:
- H1, H2
- Viktig body-tekst


### Secondary Text (Slate)

HEX:
#4A4A48

Bruk:
- Labels
- Hjelpetekst
- Metadata


### Kontrollert Sort (High Contrast)

Kun brukt når noe skal føles definitivt.

HEX:
#111111

Bruk:
- Primary button
- Kritiske handlinger
- Aktiv bekreftelse

Aldri bruk #000000.

---

### Accent Primary

HEX:
#FF2E63

Bruk:
- Primærknapp (CTA)
- Aktiv indikator
- Fokus-state
- Aktiv navigasjon
- Kryptert “flow”-visualisering

---

### Accent Hover

HEX:
#FF4D7A

Bruk:
- Hover på CTA
- Interaksjonsfeedback
- Mikro-animasjoner

---

### Accent Glow (kun i hero / visualer)

HEX:
#FF3B5C

Bruk:
- Subtil glow-effekt
- Flow-linje i hero
- Retningsindikasjon

Brukes aldri i tekst.

---

### Accent Dark Blend

HEX:
#8B1E3F

Bruk:
- Dyp blending mot mørk bakgrunn
- Subtil gradient-fade (kun i visual)
- Shadow-accent i systemgrafikk

---

## Kontrast-hierarki

#111111 = Beslutning  
#1A1A1A = Struktur  
#FF2E63 = Retning  
#F4F1EA = Rom  

Accent skal alltid ha mørk kontekst rundt seg.
Den skal føles som lys i mørket.

Det er forskjell.

---

## Oppsummeringstabell

| Element            | HEX       |
|-------------------|-----------|
| Background        | #F4F1EB   |
| Surface           | #EAE6DF   |
| Border            | #D8D2C6   |
| Primary Text      | #2B2B2B   |
| Secondary Text    | #4A4A48   |
| Primary Button    | #111111   |
| Accent            | #FF2E63   |
| Dashboard Shell   | #F7F7F8   |
| Dashboard Canvas  | #FCFCFD   |
| Dashboard Border  | #E6E6E8   |

---

Dette er et kontrollert, fysisk og arkitektonisk fargesystem.

Lite. Presist. Bevisst.

# 3. Layout Principles

## Struktur

- Maks to kolonner
- Stor margin rundt alt
- Luft mellom seksjoner
- Ingen tunge rammer
- Ingen overflødig dekor

Whitespace er en aktiv del av designet.

---

## Linjer

- 1px
- Sort eller mørk charcoal
- Kun der det gir mening
- Aldri brukt som dekor

Linjer representerer:
- Struktur
- Kryptering
- Kontroll
- Presisjon

---

## Ikoner

- Små
- Tynne strokes
- Outline style
- Ingen fyll
- Ingen 3D
- Ingen farge (kun base + accent ved aktiv)

Ikoner skal føles tekniske og presise.

---

# 4. Typography System

## Overordnet prinsipp

Typografien skal være:

- Redaksjonell
- Rolig
- Intellektuell
- Ikke-markedsførende
- Presis

Den skal føles som en bok, ikke en SaaS-annonse.

---

## Font-stack

### Primær Serif (Identitet / Editorial)
**Cormorant Garamond**

Brukes til:
- H1
- H2
- Sitater
- Filosofiske statements

Gir:
- Autoritet
- Eleganse
- Tidløshet

---

### Sekundær Sans (UI / Funksjonell)
**Inter**

Brukes til:
- Body
- Skjema
- Labels
- Navigasjon
- Metadata
- Knapper

Gir:
- Lesbarhet
- Teknisk presisjon
- Modernitet

---

# 5. Typographic Scale

## H1 – Hero

- Font: Cormorant Garamond
- Weight: 400
- Size: clamp(40px, 5vw, 72px)
- Line-height: 1.1
- Letter-spacing: -0.01em
- Farge: Almost black / charcoal

Regler:
- Kun én per side
- Aldri bold
- Aldri caps

---

## H2 – Seksjonstitler

- Font: Cormorant Garamond
- Weight: 400
- Size: 32–40px
- Line-height: 1.2

Brukes til:
- Seksjonsstarter
- Store overganger

---

## H3 – Underoverskrift

- Font: Inter
- Weight: 500
- Size: 18–20px
- Line-height: 1.4

---

## Body

- Font: Inter
- Weight: 400
- Size: 15–16px
- Line-height: 1.6–1.7
- Maks bredde: 60–70ch

Ingen justified tekst.

---

## Small / Metadata / Caption

- Font: Inter
- Weight: 400
- Size: 12–13px
- Letter-spacing: 0.02em
- Farge: Muted charcoal

Brukes til:
- Juridisk tekst
- Hjelpetekst
- Dato
- Secondary info

---

## Quote Style

- Font: Cormorant Garamond
- Style: Italic
- Size: 20–24px
- Line-height: 1.4

Brukes til:
- Statement
- Filosofisk tekst
- Brand-øyeblikk

---

# 6. Surface & Depth

- Ingen glassmorphism
- Ingen gradient-heavy UI
- Ingen hard shadow

Kun:
- Subtil, myk skygge
- Matte flater
- Tydelige kanter

UI skal føles fysisk – nesten taktil.

---

# 7. Sikkerhetsestetikk

Sikkerhet visualiseres gjennom:

- Tynne vertikale linjer
- Små nøkkel- eller lås-ikoner
- Stram grid
- Symmetri
- Rom

Ikke gjennom store skjold-illustrasjoner.

---

# 8. What We Avoid

- Bold corporate blå
- Overdesignede dashboards
- Runde, lekne kort
- Tykke borders
- Ikoner med fyll
- Neon-effekter
- Overforklarende UI

---

# 9. Brand Essence

Dette designet skal føles som:

En port.  
En terskel.  
Et rom med kontroll.  
Et sted der struktur møter stillhet.

Ikke dramatisk.  
Ikke høylytt.  
Men kompromissløst presist.
