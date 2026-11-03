# ADR: a dedicated user-content origin is deferred

**Status:** Deferred — 2026-11-03  
**Decision owner:** Frontend Plane (the sandboxed frame, the artifact viewer registry and the widget and app bridges). Control Plane (domain, certificates, cookie scope) and Application Plane (any stored app state) are consulted before any reopening.

## Context

HTML that may run script is drawn by one renderer: an iframe with `sandbox="allow-scripts"`, without `allow-same-origin`, and with a `srcdoc` document that `sandboxHtmlDocument` wraps in a strict meta CSP (`default-src 'none'`, `connect-src 'none'`, `form-action 'none'`, `frame-src 'none'`). The frame has an opaque origin, so it cannot reach cookies, storage, the app's DOM or the app's `/api` routes. It serves model-written artifacts through `ArtifactHtmlPreview` and user-attached HTML through `ChatAttachmentCanvas`. The artifact viewer registry (`apps/Frontend Plane/verevonv3/src/shared/artifacts/viewer-registry.ts`) names this frame as the only renderer for `untrusted_markup`, and on the Space room it also requires a click before drawing.

EP-4 adds chat widgets (S1), MCP App HTML (S8, S9) and app-initiated tool calls (S10). Each could instead be served from a dedicated user-content origin: a separate site with its own cookies, storage and network policy. Such an origin would let an app keep state, call its own API and reach the network. It would also need a domain, certificates, a cookie scope and CSP and CORS policies that no plane owns today.

The plan recommends MCP Apps on the existing sandboxed frame before any new origin (DEC-40, `docs/plans/model-plane-chat-build-plan-2026-10-14.md`). The EP-4 design (`docs/plans/wave3/EP-4-inline-outputs-and-mcp-apps.md`, D14) takes that recommendation for Wave 3.

The registry's claim covers artifact markup, not every frame in the Frontend Plane. These frames sit outside the registry today (paths are under `apps/Frontend Plane/verevonv3/src/`), and this decision does not change them:

- The Inbox email body (`features/inbox/components/EmailBody.tsx`) is a `srcdoc` frame with `sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"`. Its scripts are off only because `allow-scripts` is absent, so it is safe only while that stays true.
- The dashboard web view (`WebPageViewer` in `features/dashboard/home/DashboardSearchResults.tsx`) would frame search-result URLs with `sandbox="allow-scripts allow-same-origin ..."`. No code in `src` renders it today. Before it is used, its URLs need a scheme check and a check that they are not on the app's own origin.
- The PDF previews in `ChatArtifactPanel.tsx`, `ChatAttachmentCanvas.tsx` and `ArtifactOfficePreview.tsx` have no `sandbox` attribute. A generated PDF's URL is gated only by `looksLikePdfContent` (`features/chat/components/chat-artifacts.ts`); the other two take their URLs from the server's office preview and from the attachment's own preview URL.

## Decision

1. **No dedicated user-content origin in Wave 3.** Artifact, widget and MCP App markup keeps the opaque-origin `srcdoc` frame, with `sandbox="allow-scripts"` and the same CSP.
2. **No app network access.** The frame keeps `connect-src 'none'`. An app that declares network domains shows a notice and still renders; the domains are reported, not honoured (EP-4 S8, D12).
3. **No app storage.** The frame has no access to cookies, `localStorage` or IndexedDB, and no app key-value store is built.
4. **No viewer-bound connectors for apps.** An app never holds a token. Its tool calls go through the host and the gateway as the viewer, read class only (EP-4 S10, D13). App writes wait for resumable MCP approval.
5. **Only the host and the narrow bridge cross the frame.** `postMessage` is checked on `event.source` and `event.origin` (EP-4 D4, S3, S9). Nothing else crosses.

## Consequences

- An app that needs the network does not work in Wave 3. The user sees the network notice, and the rest of the app still renders.
- An app that must keep state between openings does not work. Each frame starts empty and gets its input and result from the host.
- The frame allows inline script (`script-src 'unsafe-inline'`). That is acceptable only because the frame has no ambient authority. A feature that needs ambient authority inside a frame reopens this decision.
- Widgets and apps draw only on the chat surface; shared and exported transcripts keep the code (EP-4 D2, S9). A share model that needs them live reopens this decision.
- The frames named in Context keep their current sandboxes. Changing them is separate Frontend Plane work.

## Reopen triggers

Revisit this decision with a new ADR that supersedes this stub when one of these holds:

1. EP-12's share ADR (S13) moves sharing beyond the owner's organisation (public links, cross-org sharing), or brings live widgets or app HTML into shared or exported transcripts. EP-12 keeps sharing org-only in Wave 3.
2. A customer app needs network access or storage that the frame cannot give, and the need is shown with that app.
3. A security review recommends serving user content from a sandbox domain with its own cookie jar.

A new origin needs, before approval:

- a named owner for the domain, certificates and cookie scope (Control);
- CSP and CORS policies, and cookie scopes, that let the new origin read nothing the app origin sets;
- a ZDR decision: content served from the origin passes through any CDN or log, so ZDR content must not be cached or logged there (the Zero Data Retention rule in the root `CLAUDE.md`);
- a storage design for any app state (Application);
- a security review of the origin's isolation, including subdomain takeover and cookie scoping;
- owner approval of DEC-40.
