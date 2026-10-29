import type { ParentProps } from "solid-js";
import { Suspense } from "solid-js";
import { A } from "@solidjs/router";

/** App shell: header + routed content. Kept thin — surfaces live in routes/. */
export function Layout(props: ParentProps) {
  return (
    <div class="app">
      <header class="app__header">
        <A href="/" class="app__brand">
          Aquatiq&nbsp;<span class="app__brand-dim">·&nbsp;Xero&nbsp;Onboarding</span>
        </A>
      </header>
      <main class="app__main">
        <Suspense fallback={<p class="muted">Loading…</p>}>{props.children}</Suspense>
      </main>
      <footer class="app__footer">
        OAuth&nbsp;2.0 + PKCE · access &amp; refresh tokens stay on the server
      </footer>
    </div>
  );
}
