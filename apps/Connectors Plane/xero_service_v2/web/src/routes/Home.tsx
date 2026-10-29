import { LOGIN_URL } from "../lib/api";

/** Landing surface: kick off the OAuth2 + PKCE authorization with Xero. */
export default function Home() {
  return (
    <section class="card">
      <h1>Connect your Xero organisation</h1>
      <p class="muted">
        Authorize Aquatiq to read your Xero data. You&apos;ll sign in at Xero,
        choose the organisation(s) to share, and be brought back here. The sync
        service then keeps the data warehouse up to date automatically.
      </p>

      <ol class="steps">
        <li>Sign in to Xero and pick the organisation(s).</li>
        <li>Review and grant the requested read permissions.</li>
        <li>Return here — you&apos;re connected.</li>
      </ol>

      {/* Plain anchor, NOT the router <A>: we want a real navigation so the
          browser follows the 302 chain to Xero. */}
      <a class="btn btn--primary" href={LOGIN_URL} rel="external">
        Connect to Xero
      </a>

      <p class="fineprint">
        We never see your Xero password. Tokens are stored server-side and can be
        revoked from your Xero account at any time.
      </p>
    </section>
  );
}
