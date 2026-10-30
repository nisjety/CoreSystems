import { A, useSearchParams } from "@solidjs/router";

/** Landing when the BFF redirects here after a failed/denied authorization. */
export default function AuthError() {
  const [params] = useSearchParams();

  const reason = () =>
    typeof params.reason === "string" && params.reason.length > 0
      ? params.reason
      : "The Xero authorization could not be completed.";

  return (
    <section class="card">
      <h1>Connection failed</h1>
      <p class="error">{reason()}</p>
      <p class="muted">No changes were made. You can try connecting again.</p>
      <A class="btn btn--primary" href="/">
        Back to start
      </A>
    </section>
  );
}
