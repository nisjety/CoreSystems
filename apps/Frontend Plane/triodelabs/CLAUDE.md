# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository layout

This is a single Next.js app that lives in `frontend/` — **run all commands from `frontend/`**, not the repo root. The repo root has no `package.json`; the `package-lock.json`/`yarn.lock` at the root are empty placeholders and can be ignored.

`backend/` (FastAPI + MongoDB) is leftover scaffolding from an earlier iteration of the project and is **not wired into the site**. The site is fully static/frontend-only now (see `refactor: remove backend and database dependencies` in git history). Don't build against it unless the user explicitly asks to revive a backend.

## Commands

All run from `frontend/`:

```bash
npm run dev             # start dev server (localhost:3000)
npm run build            # production build
npm start                # serve production build
npm run lint             # eslint src
npm run lint:fix
npm run type-check       # tsc --noEmit
npm run format           # prettier --write . (config: .config/prettier.config.js)
npm run format:check

npm test                 # vitest (unit tests)
npm run test:watch
npm run test:coverage
npm run test:ui

npm run test:e2e         # playwright
npm run test:e2e:ui
```

Test setup caveats: there is **no `vitest.config.*` and no `playwright.config.*`**, and no e2e specs exist yet, so `npm run test:e2e` has nothing to run. Vitest runs with defaults (node environment, no `@/*` alias resolution, no jsdom), so unit tests import via relative paths and only exercise pure logic. If you need alias imports or DOM tests, add a `vitest.config.ts` first (`@vitejs/plugin-react` and `@testing-library/*` are already installed).

Run a single unit test file: `npx vitest run src/features/services/service-seo.test.ts`
Run tests matching a name: `npx vitest run -t "some test name"`

Note: `frontend/` has both `package-lock.json` and `pnpm-lock.yaml` committed; `npm` is the documented workflow (per `frontend/README.md`), so prefer `npm` unless told otherwise.

## Architecture

- **Next.js 15 App Router + React 19 + TypeScript (strict) + Tailwind CSS v3** (`tailwind.config.ts`), bilingual: Norwegian at `/…` and a hand-written British English translation at `/en/…`. Path alias `@/*` → `frontend/src/*`.
- **Two languages, two route groups.** `src/app/(nb)/` and `src/app/(en)/en/` each have their own root layout (`<html lang>` per language, via `components/layout/root-document.tsx`); there is no shared `app/layout.tsx`, so unmatched URLs render `app/global-not-found.tsx` (`experimental.globalNotFound`). `src/lib/i18n/routes.ts` (`localizedRoutes`) maps every Norwegian path to its English slug — a new page needs a row there plus a route file in both groups. Content keeps Norwegian hrefs; components pass them through `localizeHref(href, locale)`. Copy is stored per language as `Localized<T>` (`{ nb, en }`) or as `…En` exports next to the unchanged Norwegian ones; `src/lib/i18n/translation-parity.test.ts` checks that both stay in step. The NO/EN switch is `components/layout/language-switcher.tsx`.
- **Routes are thin.** Files under `src/app/(nb)/**/page.tsx` (e.g. `om-oss`, `plattform`, `tjenester/*`, `kontakt`) render a single composed page component from `src/features/`, e.g. `src/app/(nb)/om-oss/page.tsx` just returns `<AboutPage locale="nb" />` from `src/features/about/about-page.tsx`. Add new routes by adding a feature page, not by putting markup in `src/app`. The route file does own its `export const metadata`, but builds it from typed data in the feature (e.g. `tjenester/nettbutikk/page.tsx` reads `commerceSeo` from `features/services/service-seo.ts`; legal routes read `legalDocumentsByLocale` from `features/legal/legal-content-en.ts`) rather than hardcoding strings.
- **Template pages driven by content objects:** the seven `tjenester/*` subpages all render through `features/services/service-article-page.tsx`, and the four legal routes (`vilkar`, `salgsvilkar`, `personvern`, `cookies`) all render `<LegalDocumentPage document={...} locale={...} />`. To add another page of these kinds, add a content object (the `canonical` field is a string-literal union type, so extend it) instead of a new component. New routes also need links added by hand in `components/layout/footer-content.ts` and/or `navigation-dropdowns.ts`.
- **Feature module convention** (`src/features/<name>/`): each feature separates concerns into small files rather than one large component:
  - `<name>-page.tsx` — top-level page composed of section components.
  - `<name>-section.tsx` / `<sub>-section.tsx` — individual visual sections.
  - `<name>-content.ts` or `<name>-page-data.ts` — copy/content and data kept out of JSX, usually with a co-located `*.test.ts` unit test.
  - `<name>-motion.ts` — framer-motion variant/easing definitions extracted from components, also unit-tested (e.g. `about-hero-motion.ts`, `portfolio-motion.ts`, `service-card-motion.ts`).
  - Unit tests (Vitest) target this content/data/motion logic, not component rendering — follow that pattern for new tests rather than adding component render tests.
  - `src/features/index.ts` is the barrel for the homepage sections consumed by `features/home/home-page.tsx`.
- **Shared building blocks** live in `src/components/`:
  - `layout/` — `Navigation`, `Footer`, `SectionShell` used across pages; nav menu data lives in `navigation-dropdowns.ts` (with its own test).
  - `ui/` — primitive UI (e.g. `button.tsx`) built with `class-variance-authority`, styled via the `cn()` helper (clsx + tailwind-merge) from `src/lib/utils.ts`.
  - `motion/smooth-scroll.tsx` — wraps pages to drive `@studio-freight/lenis` smooth scrolling; it lazy-loads Lenis and disables itself for `prefers-reduced-motion` and coarse-pointer (touch) devices, so treat that as the pattern for any new scroll/animation behavior.
  - `icons/index.ts` — curated re-export barrel of `lucide-react` icons; import icons from here rather than directly from `lucide-react`.
- **Global styles:** both group layouts (and `global-not-found.tsx`) import `@/styles/globals.css`. `src/app/globals.css` is a differing, unused copy — edit the `src/styles/` one.
- **Brand logos** in `features/technologies/technologies-section.tsx` come from the `simple-icons` package, not `lucide-react`.
- **`src/lib/`** holds cross-feature config: `constants.ts` (`siteConfig`, service/project/budget/timeline copy used by multiple features), `validations.ts` (zod schemas, e.g. `contactFormSchema`), `motion.ts` (shared framer-motion variants/easing), `utils.ts` (`cn`, formatting/debounce/throttle helpers).
- **Contact form is intentionally frontend-only** (`src/features/contact/contact-page.tsx`): submission is simulated (`await new Promise(...)`) with a comment noting a future form provider can be plugged in later without coupling to a custom backend. Don't wire it to `backend/` — that directory is legacy and unused.
- `next.config.ts` sets security headers (X-Frame-Options, etc.), whitelists remote image hosts (jsdelivr, framerusercontent, cision, wikimedia), and optimizes `framer-motion`/`lucide-react` imports. Deployment target is Vercel (`vercel.json`).
