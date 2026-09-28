# Combined Home updates

The Home preview now replaces its separate News and Traffic tiles with one
`DashboardUpdatesCard`. It has News / Traffic / Aquatiq pill controls and an
expanded native dialog. Selection is shared between the card and dialog;
opening or switching never sends a chat. Native dialog behavior supplies focus
containment and Escape/return-to-trigger. The older dashboard card rail stays
unchanged.

The Aquatiq view reads `/api/v1/knowledge/documents?type=sharepoint_page` through
the existing authenticated gateway and Data Plane viewer scope. It loads a
bounded 100-document window, links only to HTTPS SharePoint sources, and offers
a site filter when multiple sites occur in that window. It does not connect or
sync new sources, store page content locally, or call Graph with browser tokens.
Old workspace/feed requests are discarded.

This is currently a synced **pages** view. The connector does not retain reliable
news promotion/publication metadata, so the UI does not claim a published-news
feed. Known home/draft metadata is excluded. The current Aquatiq workspace
returned zero available pages during live verification. A specific site can be
selected once its pages are available; published-news-only behavior requires a
connector contract carrying that distinction.

Validation: 10 tests across the feed client, combined card, and existing recent
work suites; TypeScript and scoped ESLint pass. Vite build passed. Desktop and
390px mobile modal checked; Escape restores focus and there is no horizontal
overflow. News and Traffic use existing live APIs. No chat, upload, crawl, sync,
or external mutation was submitted.

To undo only this addition, remove the DashboardUpdatesCard import/render from
DashboardOverview.tsx and restore its dashboardCards filter to
`['weather', 'news', 'traffic'].includes(card.id)`. The new action, API, component,
CSS, and their tests can then be removed. Before-copy of DashboardOverview.tsx
is in `%TEMP%/verevon-home-updates-20260928`.
