# verevon-web vs. 10 Nordic + world AI competitors

Homepage design/copy comparison done 2026-07-20 against the "two trust
levers" doctrine in `CoreSystem/verevon-feature-map.md` §6.6a. Written up here
so the research and its conclusions live with the code, not only in chat
history.

## The comparison table

| | Hero headline | Trust lever | Visual style | CTA style | Pricing |
|---|---|---|---|---|---|
| **verevon-web** | "Fra signal til handling." / "Kilder, svar og godkjenning i én arbeidsflyt." | Both, but neither on the hero (before this pass — see Implementation status below). "Godkjenning" named as a process step, not marketed as a guarantee. Real sovereignty content (EU residency, ZDR, CLOUD Act caveat, live-vs-planned certs) lived on `/trust`, reached via a plain-text "TILLIT" nav label | Cinematic dark mountain-sunset photography, thin geometric sans wordmark, scroll-driven interactive product mockups (agent builder, inbox, search) | "Se arbeidsflyten" — an anchor-scroll, not a conversion action. No demo/signup/contact button anywhere on the homepage | None. No pricing page, no nav link. ("Tidlig tilgang" existed only in the footer) |
| **Ayfie** | "The AI platform for work." | Both. "Made in Norway. Private by design." (sitewide footer tagline) + Telenor AI Factory partnership banner + "Every answer, with a source" | Elegant serif display type, clean white, professional office photography, ChatGPT-style demo UI embedded in hero | "Get started" (real signup URL) + "Contact us" — mixed self-serve/sales | Published, tiered: €20/user/mo (Starter, 10–50 users) → €15/user/mo (Team, volume discount) → Enterprise from €3,990/mo, contact sales |
| **Mimir** | "The AI agent for E-commerce" | Approval mechanic, explicit. "Complete control by default. Choose between fully automated replies or AI drafts your team approves." No sovereignty claim | Bold blue gradient, starry-sky illustration accents, serif display headline | "Get a Free Demo" / "Book a Demo" — demo-gated. "Only Pay if it Works!" | Not published (outcome-based, implied custom) |
| **Cobrief** *(not a true competitor — GTM/pricing benchmark)* | "Gled deg til neste anbud" (playful, Norwegian) | Neither, explicitly — leans on social proof instead (2,500+ companies, named testimonials) + a footer Trust Center | Pastel mesh-gradient background, bold serif headline, most consumer-SaaS-feeling of the set | "Prøv Cobrief gratis" — fully self-serve, "live in 3 minutes" | Published, NOK tiers: 12,000–78,000 kr/yr → Enterprise from 150,000 kr/yr, contact sales. Most transparent pricing in the set |
| **boost.ai** | "The conversational AI platform regulated industries trust." | Both, loudly. "Built for compliance-heavy environments... security, privacy and auditability" + "hybrid control... governance and oversight" | Heavy bold sans-serif, deep purple, real city-skyline photography | "Book a Demo" / "Contact Us" — fully demo-gated, dual header CTA | None. Enterprise sales motion (4.8/5, 94% would recommend) |
| **Semine** *(now "a rydoo company")* | "Invoices that handle themselves" | Approval mechanic, explicit. "AI codes, routes, matches, and posts based on what your team approves... every decision comes with a confidence score" + dedicated Approval Workflows section. Sovereignty absent from hero, GDPR/Transparency Act only in footer | Black background, chunky grey/white sans type, dense stat bar (85% automated, 40M+ lines, 10K+ companies) + Gartner/Deloitte badges | "Schedule a demo" — fully demo-gated | None surfaced |
| **Simplifai** | "Simplifai ↘ Insurance — AI Agents for Insurers" | Neither, on the hero — ROI/case-study social proof instead (25x ROI at Storebrand); GDPR/ISO/Transparency Act relegated to footer legal links | Deep purple/navy, serif display headline, real human video testimonial in hero (not stock/illustration) | "Start Your AI Journey" / "Book a demo" — sounds self-serve, almost certainly form-gated | None surfaced |
| **Sana Labs** | "Superintelligence for work" | Neither. Relies on enterprise logos (Merck, Apollo) + a Spotify quote; "Security" is an unexplored footer link | Extreme minimalism, huge whitespace, black sans-serif on white, zero photography/illustration | "Book an intro" — fully demo-gated | Gated behind a country-selector quote form, no published numbers |
| **Sanity.io** *(not a true competitor — Oslo-founded dev-tool PLG reference)* | "The Content Operations Platform" / "Power content applications and AI workflows at scale." | Sovereignty, and uniquely proven, not planned — SOC 2 Type II, GDPR, CCPA stated as achieved certifications, plus ">99.95% uptime" | Black background, huge bold display type, an actual live interactive CMS product mockup embedded in the hero (clickable studio UI, real TS schema code, npm command) | `npm create sanity@latest` shown directly in the hero + "Start building" — pure self-serve PLG | Published, freemium: Free ($0) → Growth ($15/seat/mo) → Enterprise (custom) |
| **Aim** (prokom.no) *(not a true competitor — govtech/CMS sovereignty-claim reference)* | "Skap fleksibelt innhold!" | Sovereignty, blunt and unqualified — "Aim CMS er en 100 % norskeid løsning," no hedging | Light background, teal blob accent, bold serif headline, real dashboard screenshot. Note: broken icon font in nav (`keyboard_arrow_down` renders as literal text) — a visible live QA bug | "Hvorfor velge Aim" — soft, informational, no demo/signup CTA | None. Public-sector/tender pricing implied (19+ named municipalities) |
| **Taito.ai** *(EU reference)* | "Run people ops on autopilot" | Sovereignty, proven — "ISO 27001 certified" + "GDPR compliant" + "EU data residency" as 4 clean badge tiles mid-page | Cream background, bold black grotesk, real product-UI screenshot in hero | "Join waitlist" — low-friction early access | Pricing nav link exists; pre-launch, no numbers live |
| **Wonderful.ai** *(EU reference)* | "Applied AI for the enterprise" | Neither on hero — quantified enterprise case studies instead (91.5% containment, 2.5M clients) | Dimmed real call-center photography, centered humanist sans | "Get in touch" / "Learn more" — fully demo-gated | None |
| **Attio** *(world reference)* | "Welcome to agentic revenue." | Neither | Huge bold black sans, live interactive CRM UI embedded in page | "Start for free" (no card) + "Talk to sales" | Published, freemium: Free → $29–36 → $69–86 → Enterprise |
| **Peec AI** *(world reference)* | "AI search analytics for marketing teams" | Neither — "Trusted by 2,500+ marketing teams" | Bold black sans, real dashboard screenshot | "Talk to Sales" + "Start Free Trial" | Published: €85 → €205 → €425/mo → Enterprise |
| **Intercom** *(world reference)* | "The only helpdesk designed for the AI Agent era" | Neither — scale + named logos (Anthropic, Clay, Lightspeed) + "Fin Million Dollar Guarantee" | Playful hand-drawn illustration + candid photo crops | "Start free trial" + "View demo" + "Contact sales" | Published: from $0.99/resolution + per-seat |
| **Chatbase** *(world reference)* | "AI agents for magical customer experiences" | Vague/generic — no named certs | Warm gradient product mockup, bold black grotesk | "Book a demo" + "Build your agent for free" | Published, freemium: $0 → $32 → $120 → $400 → Enterprise |
| **Gorgias** *(world reference)* | "Conversations that drive revenue, not just resolutions." | Neither — "40% of Shopify brands" + guardrails/escalation framing | Real street-fashion lifestyle photography, live chat-widget overlay | "Discover pricing" + "Book a demo" + "Sign up free" | Outcome-based, no flat numbers shown |
| **Zendesk** *(world reference)* | "Move beyond deflection. Deliver real resolutions." | Neither — scale stats (22K+ teams, 4.8B resolutions) | Bold sans, green accent word, moody real photography | "Try for free" (14-day, no card) + "View demo" | Tiered, self-serve entry confirmed |

**Pattern:** every Nordic/EU competitor states its trust lever (or leans on
named social proof as a substitute) in the first screen. All 6 "world"
competitors instead pair a self-serve entry point with published pricing —
confirming the sovereignty lever is a Nordic/EU-buyer concern specifically,
not a global default. Verevon was the one site in the whole set where the
trust lever was real and well-documented but invisible until a visitor
clicked a muted nav label three levels deep.

## Verevon's Trust Center was (and is) a hidden asset

`/trust` is the most rigorous trust page in the entire set. It names
live-vs-planned controls individually, lists actual subprocessors with
per-vendor ZDR/region status, and — unlike every competitor — actively
undercuts its own sovereignty claim: *"EU-residens er ikke det samme som
datasuverenitet... kan nås under den amerikanske CLOUD Act"* and *"Vi har
kontrollene — men ennå ikke sertifiseringene."* No competitor admits a gap
like that. Real differentiator, just previously unmarketed.

## Verdict

**Doing right:** the cinematic photography is the single most premium visual
asset in the set (nobody else uses real landscape photography); the
scroll-driven interactive product storytelling shows the actual workflow
instead of describing it; the underlying trust content outmatches every
competitor's on rigor and honesty, once found.

**Was weak or missing relative to the pattern:** zero trust-lever claim at
hero level; zero conversion CTA on the homepage; zero pricing signal or path
to one; zero customer social proof; the hero image did no work explaining
the product category.

## Recommendations — final, minimal, premium-preserving

The first pass proposed 5 changes, including a UI-mockup or logo wall in the
hero — exactly the generic-SaaS move that would have made Verevon look like
this entire competitor set instead of itself. Replaced with 3 tighter moves,
each reusing an existing component:

1. **One eyebrow-line**, reusing the site's own tracked-uppercase label motif
   (`01 / PROBLEMET`, `TRUST CENTER`) — pulled verbatim from `/trust`'s own
   copy, not invented.
2. **A second ghost-link**, identical style to the existing CTA, surfacing
   the "Tidlig tilgang" concept that only lived in the footer before.
3. **One named partner line**, not a logo wall — in the same muted micro-type
   as the existing "Scroll" hint.

**Explicitly dropped:** changing or augmenting the hero image with a product
screenshot — every "world" competitor does this; the landscape photography
is an asset, not a gap.

## Implementation status (2026-07-20)

Shipped in this pass:
- `src/components/home/sections/HeroSection.tsx` — added the `Eyebrow`
  ("EU-residens som standard", reused verbatim from `/trust`) above the H1,
  and a second CTA link ("Be om tidlig tilgang", `href="#kontakt"`) next to
  "Se arbeidsflyten." Both verified in desktop + mobile viewports, `pnpm
  lint` clean.
- `src/components/core/footer/Footer.tsx` — added "Partnere: Aquatiq ·
  Prokom" to the existing office-facts block. Both are free/early-access
  testing partners, not paying customers — "Partnere" is accurate as
  worded (real, close collaboration) as long as it never implies a
  commercial/"trusted by" relationship it doesn't have yet.

**Open, not yet done:** whether to extend `BrandLogosSection`'s "Koblet til
systemene deres" carousel with Vipps, Posten/Bring, UPS, DHL, FedEx, Meta, X,
LinkedIn (real adapter code confirmed via grep in `auth-core`/`shipping-
core`/`social-core`/`leads-core`) and drop Altinn (zero code found anywhere).
That component has its own explicit "honesty gate" code comment — only
genuinely *live* connectors belong in it, not just "adapter code exists."
Needs confirmation that these are credentialed/live in production (not mock
fallback) before adding, given a past note that Bring specifically was
"unverified" in an earlier audit.

**Not pursued, per Telenor AI Factory note above:** upgrading the eyebrow
copy from "EU/EØS" to a Norwegian-infrastructure claim — only once that deal
closes, not before.
