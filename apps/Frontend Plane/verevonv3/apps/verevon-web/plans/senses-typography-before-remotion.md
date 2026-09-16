# Senses: typografien rett før Remotion

Gjenfunnet i denne samtalens verktøylogg 15. september 2026. Dette er den lokale versjonen med «Løs oppgavene som driver bedriften fremover.» og de godkjente tekstene om delegering, læring og kontroll.

## Originale verdier

| Element | Skrift | Vekt | Størrelse | Linjehøyde | Bokstavavstand |
| --- | --- | --- | --- | --- | --- |
| Hovedoverskrift og alle tre kapitteloverskrifter | Arbeit Pro Light | 300 | `clamp(1.45rem, 3.6vw, 4.95rem)` | 0.88 | -0.08em |
| Samme overskrifter ved bredde ≤ 760 px | Arbeit Pro Light | 300 | `clamp(1.75rem, 7.2vw, 2.7rem)` | 0.98 | -0.06em |
| Ingress | Arbeit Pro Light | 300 | `clamp(1.04rem, 1.53vw, 1.4rem)` | 1.42 | -0.025em |
| Kapitlenes brødtekst | Arbeit Pro Light | 300 | `clamp(1rem, 1.06vw, 1.22rem)` | 1.62 | -0.02em |
| «Kapittel 01 / Delegering» og tilsvarende | Protokoll Light | 300 | 0.68rem | 1 | 0.26em, versaler |
| Lenken under første kapittel | Protokoll Light | 300 | 0.88rem | Arvet | Arvet |
| «Mer kapasitet. Flere muligheter.» | Arbeit Pro Light | 300 | `clamp(1rem, 1.06vw, 1.22rem)` | 1.62 | -0.02em |

Med 16 px grunnstørrelse:

| Element | 1329 px bredde | 1567 px bredde |
| --- | ---: | ---: |
| Hoved- og kapitteloverskrifter | 47.844 px | 56.412 px |
| Ingress | 20.3337 px | 22.4 px |
| Brødtekst og avslutning | 16 px | 16.6102 px |
| Kapitelletiketter | 10.88 px | 10.88 px |
| Lenke | 14.08 px | 14.08 px |

## Komposisjon og tekststil

- Hovedoverskriften brukte `verevon-home-heading`, med maksimal bredde 26ch og de to godkjente linjene.
- Kapitteloverskriftene brukte samme klasse, maksimal bredde 14ch på desktop og 11ch under 1024 px.
- Ingressen hadde maksimal bredde 558 px, 28 px toppmargin og dempet tekstfarge ved 66 %.
- Kapitlenes brødtekst var sentrert på desktop, maksimal bredde `min(540px,40vw)` og dempet tekstfarge ved 66 %. Utvalgte formuleringer hadde terrakottatone og vekt 400.
- Den faste tekstgruppen brukte avstand `clamp(22px,2.5vw,42px)` mellom elementene.
- Avslutningen var liten, høyrestilt brødtekst, med maksimal bredde 540 px. Den var ikke en stor todelt overskrift.

## Hva Remotion-oppdateringen endret

- Kapitteloverskriftene fikk `clamp(38px,4.1vw,65px)`, linjehøyde 1.04 og bokstavavstand -0.055em.
- Brødteksten fikk vekt 400, størrelse `clamp(16px,1.22vw,19px)` og linjehøyde 1.65.
- Ingressen fikk vekt 400, størrelse `clamp(17px,1.53vw,22px)`, linjehøyde 1.5 og maksimal bredde 650 px.
- Avslutningen fikk `clamp(28px,3.2vw,48px)` og et eksplisitt linjeskift.
- Kapittelmerker og lenke fikk andre skriftinnstillinger.
- Hovedoverskriften beholdt `verevon-home-heading`.

## Kildegrunnlag

Fra denne oppgavens lokale samtalelogg, uten å basere rekonstruksjonen på Git-versjonen:

- 14. september 2026 kl. 20:19:52 UTC: kildefilen ble lest og de daværende typografiklassene ble lagret i verktøyresultatet (post 156 i sesjonsfilen fra 14. september).
- 14. september kl. 20:21:10 UTC: tekstpatchen innførte «Løs oppgavene …», «med dere som dirigenter» og «Verevon lærer». Den beholdt typografiklassene (post 162).
- Delte `verevon-home-heading`-verdier er dokumentert i loggens CSS-avlesning fra 13. september kl. 22:22:00 UTC (post 3562 i sesjonsfilen fra 13. september kl. 01:40).
- 14. september kl. 21:28:23 UTC: Remotion-integrasjonen erstattet strukturen og opprettet `SensesSection.module.css` med de nye størrelsene (post 408).

## Gjeninnført 15. september 2026

Verdiene er nå gjeninnført i `SensesSection.tsx` og `SensesSection.module.css`. Kapitteloverskriftene bruker igjen den delte `verevon-home-heading`-klassen, og ingress, brødtekst, kapittelmerker, lenke og avslutning bruker verdiene over. Den godkjente teksten er beholdt, inkludert «med dere som dirigenter», kontrollen hos teamet og at «Verevon lærer».

Kontrollert i nettleseren ved 1567 px og 390 px bredde: beregnede skriftverdier stemmer, mobilvisningen har ingen horisontal overflyt, og alle tre kapitlene beholder den opprinnelige scrollovergangen med Remotion-elementene. Fokusert ESLint og TypeScript-kontroll fullførte uten feil.
