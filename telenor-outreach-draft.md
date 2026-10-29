# Telenor AI Factory — outreach draft (Aquatiq AS)

Two pieces. **Send A only.** B is the question list for the first call — asking
these in a cold intro reads like an RFP from a company they have not qualified
yet, and every one of them is a reason for a slow "let me check with legal"
instead of a meeting.

Placeholders in `[ ]` are things only you know. Voice follows the Aquatiq brand:
clear, confident, expert, warm — a knowledgeable partner, not a hype machine.

---

## A — First contact (web form "Request early access", or email)

Norwegian, plain text. Deliberately not a branded HTML mail: a first approach to
a Norwegian telco should read as a person writing, not as marketing.

> **Emne:** Tidlig tilgang — AI for mattrygghet, med databehandling i Norge
>
> Hei,
>
> Aquatiq leverer komplette løsninger og fagkompetanse innen mattrygghet. Vi
> bygger nå AI-funksjonalitet inn i tjenestene våre: verktøy som arbeider direkte
> mot kundenes egen dokumentasjon — HACCP-planer, avviks- og revisjonsfunn,
> prosedyrer, analyseresultater — og som hjelper fagfolk å finne svar i eget
> materiale raskere. Plattformen er i drift i dag, med inferens på Azure i EU.
>
> Det er derfor vi tar kontakt. Innholdet vi behandler er kommersielt sensitivt
> for kundene våre: et revisjonsfunn, et avvik eller en prosessparameter knyttet
> til en navngitt produsent er ikke noe som skal ut av landet uten videre. Våre
> kunder er i en regulert bransje, og spørsmålet om hvor data faktisk behandles
> kommer alltid. EU-residens på Azure er en kontraktsfestet garanti — ikke en
> fysisk en — og den forskjellen er merkbar i en anskaffelsesprosess.
>
> Vi ønsker derfor et eget nivå i plattformen der data behandles og lagres i
> Norge, på norsk infrastruktur, for de arbeidslastene som krever det. Åpne
> vektmodeller dekker behovet godt på det nivået.
>
> Konkret ser vi etter:
>
> - inferens på åpne vektmodeller (Llama-, Mistral- og Qwen-familiene, samt
>   nordiske modeller) i norske datasentre
> - en teknisk gjennomgang av hva plattformen faktisk tilbyr — om vi drifter
>   vLLM selv på OpenShift, eller om det finnes et forvaltet inferens-endepunkt
> - hva et realistisk oppstartsnivå ser ut som for et selskap på vår størrelse
>
> Vi starter smått og skalerer med faktisk etterspørsel. Lykkes dette, tar vi med
> norske matprodusenter inn på plattformen — ikke bare én leietaker.
>
> Har dere kapasitet til en teknisk samtale de neste par ukene?
>
> Vennlig hilsen
> [Navn]
> [Rolle], Aquatiq AS — org.nr [nummer]
> [telefon] · [e-post] · aquatiq.com

### English version, if they need to forward it internally

> **Subject:** Early access — AI for food safety, with data processed in Norway
>
> Hi,
>
> Aquatiq supplies complete food safety solutions and expertise. We are now
> building AI into our services: tools that work directly against customers' own
> documentation — HACCP plans, deviation and audit findings, procedures, lab
> results — helping specialists find answers in their own material faster. The
> platform is in production today, with inference running on Azure in the EU.
>
> Which is why we are writing. What we process is commercially sensitive to our
> customers: an audit finding, a deviation, or a process parameter tied to a named
> producer is not something to move out of the country lightly. Our customers work
> in a regulated industry, and the question of where data is actually processed
> always comes up. EU residency on Azure is a contractual guarantee rather than a
> physical one, and that distinction is felt in procurement.
>
> So we want a distinct tier in the platform where data is processed and stored in
> Norway, on Norwegian infrastructure, for the workloads that require it.
> Open-weight models cover what we need at that tier well.
>
> Specifically we are looking for:
>
> - inference on open-weight models (Llama, Mistral and Qwen families, plus Nordic
>   models) in Norwegian datacentres
> - a technical walkthrough of what the platform actually provides — whether we
>   operate vLLM ourselves on OpenShift, or whether a managed inference endpoint
>   exists
> - what a realistic entry commitment looks like for a company our size
>
> We would start small and scale with real demand. If this works, we bring
> Norwegian food producers onto the platform with us — not just one tenant.
>
> Would you have capacity for a technical conversation in the next couple of weeks?
>
> Best regards,
> [Name]
> [Role], Aquatiq AS — org.nr [number]
> [phone] · [email] · aquatiq.com

---

## B — Questions for the first call (do NOT send with A)

Grouped by who answers them. Ask the technical block first: it decides whether
this is viable at all, and the commercial questions are wasted effort if it isn't.

### Technical — decides feasibility
1. Is there a **managed inference endpoint**, or do we deploy and operate vLLM /
   KServe on OpenShift ourselves? If the latter: who owns capacity planning,
   autoscaling and model upgrades?
2. Is the inference surface **OpenAI wire-compatible**? (Decides whether adding
   you is a base-URL registration or a new adapter for us.)
3. The "190+ models" — is that a catalogue we deploy *from*, or endpoints you
   serve? Which specific models are available as served endpoints today?
4. Can we run **Nordic-language models** (Bineric's, NorwAI's, or our own
   fine-tunes) on the platform? Norwegian-language quality matters to us more
   than raw benchmark scores.
5. **Cold-start and scale-to-zero**: what happens to a reserved GPU between
   requests? Our load is bursty and interactive, not batch.
6. What is the **network path** — public endpoint, or IPVPN / Nordic Connect
   only? Latency budget from an EU-hosted application?

### Compliance — decides whether we can sell it
7. Do you provide an **Art. 28 databehandleravtale**? Can we see the template?
   We need to name you as a sub-processor in our own customer agreements.
8. Where exactly do the datacentres sit, and is the **sub-processor list**
   published? (Accenture is named as co-operating the platform and Red Hat
   supplies the control layer — both belong in that list.)
9. **ISO 27001 / SOC 2** — certified, in progress, or not on the roadmap? With
   what target date? Our customers' own audit regimes will ask.
10. May we **name Telenor AI Factory in customer-facing material** as the
    underlying infrastructure for our Norwegian tier?
11. Is there anything in the platform's design that supports customers subject to
    **sikkerhetsloven**? (Telenor Norway's own regulated workloads reportedly run
    there — how much of that posture is available to a third-party tenant?)

### Commercial — only if the above clears
12. **Pricing model**: on-demand GPU-hour vs reserved. What are the actual rates,
    and what is the minimum term?
13. **SLA and support**: the site indicates weekday business hours, best-effort
    during the initial phase. What does GA look like, and is 24/7 available?
14. Is there a **status page** or incident-communication channel?
15. What does the **onboarding path** look like — how long from signed agreement
    to first inference request?

---

## C — Melding til kontakten din i Telenor (varm rute)

Send denne **i stedet for** skjemaet, og hold A klar til videresending. Kort med
vilje: en selger som skal gjøre deg en tjeneste skal bruke to minutter, ikke tjue.

> Hei [Navn],
>
> Har du fem minutter til et internt spørsmål?
>
> Vi i Aquatiq ser på **Telenor AI Factory** — den norske AI-/GPU-plattformen som
> ligger under telenoraifactory.no. Vi bygger AI inn i mattrygghetstjenestene våre,
> og trenger et nivå der kundedata behandles i Norge og ikke i EU-skyen. Det er
> nøyaktig det den plattformen er laget for.
>
> Jeg trenger ikke at du selger noe — jeg trenger å komme til rett person. To
> spørsmål:
>
> 1. Vet du hvem som eier AI Factory kommersielt, og kan du introdusere oss?
> 2. Selges den gjennom kundeteamene deres, eller direkte fra AI Factory-teamet?
>
> Jeg har en ferdig henvendelse du bare kan videresende hvis det er enklest — si
> ifra og jeg sender den over.
>
> Takk!
> [Navn]

### Hvorfor den er formet slik

- **"Jeg trenger ikke at du selger noe"** er den viktigste setningen. Den lar dem
  hjelpe uten å måtte ta eierskap — og hindrer at henvendelsen blir *deres* deal
  og går inn i en pipeline den ikke hører hjemme i.
- **Spørsmål 2 er reell etterretning**, ikke høflighet. Svaret avgjør om du i det
  hele tatt skal gå gjennom kundeteam senere, eller alltid direkte.
- **Du tilbyr den ferdige e-posten, men dytter den ikke på dem.** Da slipper de å
  skrive noe selv, uten at du har sendt dem en vegg av tekst de må lese først.
- **Ikke send A som vedlegg med en gang.** Hvis de sier ja til å videresende, får
  de A — og da er den allerede "godkjent" internt, som er hele poenget med å gå
  denne veien.

### Tidsvindu

Gi dem **omtrent en uke**. Hører du ingenting, send A gjennom skjemaet og nevn at
du også har snakket med [Navn] — da er den varme sporen fortsatt verdt noe uten at
du blir sittende og vente. Ikke kjør begge sporene samtidig fra dag én: to
henvendelser om samme sak fra samme selskap ser ukoordinert ut.

---

## Notes on choices in this draft

**Aquatiq AS is the sender; the platform name is deliberately quiet.** Telenor
contracts with a legal entity in a named industry, and "food safety company with
regulated customers" is a far more legible counterparty than a product name they
have never heard. If you want the product named, add it once in the first
paragraph — but it earns nothing in a first contact.

**The sovereignty argument is concrete rather than generic.** "Personopplysninger"
is what everyone says. *An audit finding tied to a named producer* is a specific,
recognisable commercial risk, and it is the version a Telenor solutions architect
will remember. This is the single biggest improvement over a generic SaaS framing.

**"We bring an industry, not a tenant" is the closing hook.** Telenor's public
references are their own security-act workloads and a DFØ public-sector pilot —
they are visibly building a regulated-industry story. Norwegian food producers
fit that, and a partner who brings a vertical is worth more to them than a partner
who rents GPUs. Only keep this line if you are genuinely willing to say it.

**Bineric is deliberately not mentioned.** They are a named Telenor customer, so
raising them proves homework — but it also invites *"just go via Bineric then,"*
which forecloses the direct relationship you want as the eventual destination.
Hold it for the call and use it as a reference question instead: *"how do partners
like Bineric consume the platform?"* — same information, without volunteering
that you have an alternative to them.

**No volume or timeline is committed**, and no customer is named. If a named
customer *is* driving this, one sentence saying so is the strongest addition
available and it changes their prioritisation.

**Check before sending:** that the HACCP / avvik / revisjonsfunn / analyseresultater
list matches what your AI features actually touch today. It reads specific and
credible, which means it will be believed — so it should be true.
