# Space cockpit: what's ready to wire in

Date: 2026-08-13
For: whoever owns `src/features/spaces/components/SpacePage.tsx`
From: the QM-improvement workstream (`apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md`,
`apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md`)

Three components are committed and tested against the Space home spec in
`VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`. **`SpacePage.tsx`,
`spaces-client.ts` and `App.tsx` are untouched** — they were uncommitted while
this was built, so nothing here edits them. Adopting it is your call and your
diff.

| File | What it is |
|---|---|
| `src/features/spaces/components/SpaceCockpit.tsx` | The six-tab shell: Samtaler, Arbeid, Kunnskap, Aktivitet, Agent, Medlemmer |
| `src/features/spaces/components/SpaceActivityFeed.tsx` | Activity rendered with the Verb/Object/Outcome grammar |
| `src/features/spaces/lib/activity-grammar.ts` | The grammar itself — pure, no SolidJS, 24 tests |

46 tests green across the four files, including your existing `SpacePage.test.tsx`.

## Wiring

```tsx
import { SpaceCockpit } from './SpaceCockpit'
import { SpaceActivityFeed } from './SpaceActivityFeed'

// replacing the current #members / #activity sections:
<SpaceCockpit
  tabs={{
    aktivitet: <SpaceActivityFeed threads={threads()?.threads ?? []} />,
  }}
/>
```

That is the whole integration. Every other tab renders its own honest
"not published yet" state until you pass it something.

## Three things to know before you change it

### 1. It fetches nothing, on purpose

Every tab's content is a prop. This isn't generic good taste — it's a response
to the surface moving underneath: `/api/v1/spaces/:space_ref/membership` was
replaced by `/actions` while this was being written. A shell that fetched would
break each time that happens, and would duplicate the client you own. It also
keeps the BFF a proxy: nothing here asks it to compose a cockpit.

Please keep it that way. If a tab needs data, load it in `SpacePage` and pass
it down.

### 2. The unavailable state is load-bearing — please don't replace it with a generic empty state

Only Chat and Activity have a real space-scoped source today; Agent gained one
with the actions catalog. **Work, Knowledge and Members have no space-scoped
endpoint at all.**

So an unsupplied tab names the plane that hasn't published a Space projection
yet, and says the gap is *a missing connection, not an empty room*. That
wording is deliberate. A tab showing a tidy "Ingen elementer" reads as "this
room has no work", which is false — the truth is "nobody has wired this yet".
The difference is exactly the failure mode both our plans identify as this
codebase's recurring one, and it's the same honesty your own `SpacePage`
already applies to run receipts.

### 3. Tabs are hash-driven so no router change was needed

`#arbeid`, `#aktivitet`, and so on. Deep links work today and your existing
`#members` / `#activity` anchors keep working. When `/spaces/:spaceId/work`
lands, swap the internal signal for a route param — nothing else in the
component changes.

## A trap that cost an hour, so it doesn't cost you one

The panel first used `<Show>`'s callback form:

```tsx
<Show when={props.tabs?.[active()]}>{(content) => content()}</Show>
```

That callback only re-runs when the condition crosses falsy→truthy. Switching
between two tabs that **both** had content left the first tab's panel on
screen: the condition stayed truthy, so the callback never re-ran. It now reads
the content through a memo and renders it as a plain JSX expression. Worth
knowing if you build any other one-of-N panel here.

## The activity grammar's extension point

`SpaceActivityItem` is deliberately **not** `SpaceThread`. Today the only Space
activity any plane publishes is a conversation with a latest-run status, so
`activityFromThread` is the single adapter.

When run receipts, approvals, and tool steps start arriving in a Space
projection — which `SpacePage` already promises its reader — add an adapter
that produces `SpaceActivityItem`s and the renderer is untouched. Run statuses
are mapped from the vocabulary session-core actually emits; anything unmapped
renders verbatim as a generic row rather than being dressed up as understood.

Ordering is by consequence, not recency: a day-old failed run sorts above a
minute-old completed one, because the question a supervisor asks is "what needs
me", not "what happened last".

## Two open questions for you

**Language.** `SpacePage.tsx` is English ("Members", "Activity", "Space
unavailable"); the rest of the app is Norwegian ("Planlagte kjøringer",
"Ferdigheter", "Minne") and these components follow that. Worth settling
deliberately rather than by whoever edits last.

Related: the product term. `Space` has won in code (`SpaceRef`,
`spaces-client.ts`, the route) and shouldn't change. But **"Rom"** may be the
better user-facing word — it carries the Slack/Teams mental model in one
Norwegian syllable, where "Space" reads as jargon. These components say "rom"
in prose and keep `Space` in identifiers.

**Roster.** The Members tab is a shell because there's no endpoint behind it —
`/actions` replaced `/membership`, and nothing returns more than the caller's
own role. Whenever S1.2's membership list surfaces, that tab is a
`<SpaceMemberList members={...} />` away.

## Not fixed, on purpose

`SpacePage.tsx` has two lint errors — `Array#map` instead of `<For>` at lines
60 and 87 (`solid/prefer-for`). Your file, your call; flagged only because
`pnpm lint` currently reports 2 errors and they're both there.
