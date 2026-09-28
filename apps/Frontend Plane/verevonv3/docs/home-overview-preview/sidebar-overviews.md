# Home sidebar overviews — 2026-09-28

Within the Dashboard overview preview, every entry in the Home sidebar selects
an overview on `/dashboard`. The narrow rail still opens each full workspace.

The selection is a validated `overview` URL parameter, so Back/Forward and
direct links work. Unknown values fall back to Home. The Home sidebar stays
open, highlights the selection, and opens the matching My account / Shared
with me group for a deep link. The breadcrumb names the selected overview.
The existing chat composer stays mounted, preserving drafts and attachments.

## Contents

| Overview | Existing source | Scope of the displayed data |
| --- | --- | --- |
| Home | Existing dashboard | Recent chat work and live information cards |
| Rom | `listSpaces` | Accessible rooms and their lifecycle |
| Studio | `listStudioProjects` | Workspace projects, including temporary-storage disclosure |
| Support | `listConversations` | Six recent conversations, explicitly not a workspace total |
| Social | `getSocialCalendar` | Posts and connected accounts |
| Knowledge | `listSources` | Available source summaries |
| Insights | `getInsightsOverview` | Recorded scorecards; no synthetic metrics |
| Leads | `listLeadLists` | Saved company lists |
| Agents | `listSystemRuns` | Six scheduled/background runs, explicitly labelled |

Only the selected area loads. Each read uses its existing session-scoped client.
Responses from a previous user, organization or selection cannot replace the
current view. Failures show a retry state with no invented zero metrics. Empty
states are separate. Explicit links in each overview open the full workspace;
row links are offered only for existing detail routes.

The Web and Extract changes remove their outer padding, borders and grey
container. Input fields, mode/profile/privacy controls, upload and knowledge
navigation remain intact. Their rollback is separate from the sidebar work.

## Verification

- 12 targeted tests pass: all sidebar selections, deep links, invalid values,
  Back/Forward, preview rollback, draft/attachment retention, retry, old-workspace
  response isolation, and owner-data summary semantics.
- Typecheck and production build pass. New/changed dashboard and action files
  pass lint. The two existing shell files retain five pre-existing lint warnings.
- Live browser checks cover all eight added overviews, including populated and
  empty states, plus the Web and Extract controls without their grey containers.
- The Studio overview was checked at 390 px with no horizontal overflow;
  the temporary viewport override was reset after verification.
- No messages, uploads, searches, crawls or workspace mutations were submitted.

## Independent rollback

Turn off **… → Appearance → Dashboard overview** to compare with the original
Home and its sidebar links. To remove only this pass, use these patches from
`C:/dev/CoresSystem` with `git apply -p1 --reverse --check` first, then the same
command without `--check`:

```powershell
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/home-overview-preview/sidebar-overviews.patch"
git apply -p1 --reverse --check --directory="apps/Frontend Plane/verevonv3" "apps/Frontend Plane/verevonv3/docs/home-overview-preview/tool-containers.patch"
```

The two patches are independent. Reverse these before the earlier
`copy-refinement.patch` and `implementation.patch` when removing all passes.
They contain only this request's changes, relative to before-edit copies.
Source backups and screenshots live under
`C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/verevon-home-sections-20260928`.
