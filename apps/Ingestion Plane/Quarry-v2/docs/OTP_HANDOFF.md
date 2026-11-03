# E3.1-j: login-fixture OTP evidence (runbook)

Unit: W3-E3.1-j (W3, step S11, lane d, branch `w23/lane-d`). Scope: the evidence script `scripts/evidence/otp-login-fixture.sh` and this page.

**Written, not run.** Nothing here has been executed, syntax-checked or compiled by the unit that wrote it. The integration step owns the compile guard. Nothing here is evidence yet.

Run every command from `apps/Ingestion Plane/Quarry-v2`.

## 1. What a pass shows

For one run that the user approved once:

1. Quarry read the one-time code from the recorded mailbox. The mailbox log shows the inbox reads.
2. The code went into the field through trusted input, and the login fixture accepted it. The fixture log shows `POST /otp-login 303`, then `GET /dashboard 200`.
3. The code's digits are in no receipt of the run, in no service log you pass in, and in no stored artifact you pass in. These are text scans.
4. The completed fill names one sensitive approval grant, and its action holds only ids: no value, code or text.

A pass does not show what a text scan cannot see: pixels, the model's transcript, the card's wording, the replay refusal, the audit event, and any ZDR run. Section 7 lists those checks, to be made by hand.

## 2. Blockers: the chain does not run end to end yet

Checked in the code at this commit. This unit changes none of them.

| # | Blocker | Where | Closed by |
|---|---|---|---|
| B1 | integration-core does not route `mail.otp.read`. `SetOTPAuditPublisher` is never called, `executeConnectionAction` does not fill `ExecuteInput.Caller`, and neither the capability map nor the bearer middleware admits `integration:otp:read` on this route. The middleware refuses a service token that holds only that scope (`integration_scope_required`) until the route admits it. | `internal/api/server.go`, `internal/auth/middleware.go` (about line 243) | E3.1-g items 1 to 4 |
| B2 | Personal connections: a quarry-edge `ingestion` token without `acting_user_id` is refused on a personal Outlook or Gmail connection. A dev user's own mailbox is probably personal. | the `mail.otp.read` owner check | E3.1-g item 5: a decision |
| B3 | The edge never builds `IntegrationCoreOtpSource`. There is no config field for the integration-core URL, and nothing calls `with_enabled(fill_otp_enabled())`. | `otp_ic_client.rs`, edge config | E3.1-h wiring |
| B4 | No executor. Nothing implements `OtpFillTarget` over the Chromium driver, and the runtime arm for `fill_otp_ref` in `observation.rs` returns `unsupported` ("no executor on this runtime"). `deliver_otp` has no caller. | `observation.rs`, `otp_source.rs` | E3.1-c and E3.1-h wiring; depth D-E31c-1 |
| B5 | The approval card is not mounted. `OtpApprovalNotice` has no caller, so the generic card shows the page URL and the selector, not the frame or the connection. | `src/features/browser/approval` | E3.1-i and the swap unit (W3-E3.2-b-swap, not built) |
| B6 | The runtime refuses http outside `cfg!(test)` (D-E31c-3). The Rust test site is in-process only, with no standalone entry point. `--serve-fixture` is a stand-in with the same markup, and it needs a dev certificate that the stack's browser trusts. | `chromiumoxide.rs`, `test_site.rs` | E3.1-c follow-up; depth |
| B7 | The recorded mailbox follows the shapes that integration-corev2 parses. It is not checked against live Graph or Gmail responses, and integration-core's token refresh, if it refreshes, is not covered. | `mail_otp.go`, the Gmail helpers in `service.go` | unverified |

Until B1 to B4 close, the chain stops before the mailbox read or before the fill. `--self-test` and `--validate-recording` run without any of them.

Not a blocker, but open: integration-core accepts a `NoopPublisher` for the audit. Only a nil publisher is refused, in `mail_otp.go`. The production publisher is NATS.

## 3. Preconditions

- **A side-by-side stack, never the live stack.** The environment in this section reaches integration-core and quarry-edge on that stack only.
- **A seeded, non-ZDR dev organisation.** A ZDR run stores no receipts today (`agent_routes/receipts.rs`, and `receipt_plan` in `human_browse.rs`), so check 1 cannot pass on one.
- **Flags, on the side stack only:** `QUARRY_EDGE__FILL_OTP=1` (the edge and the browser driver read it) and `INTEGRATION_OTP_READ=1` (integration-core). Both default off. Never set them on the live stack.
- **Provider base URLs, on the side stack only:** `MICROSOFT_GRAPH_BASE_URL=http://<bridge host>:<MAILBOX_PORT>` for a Microsoft connection, or `GOOGLE_API_BASE_URL=http://<bridge host>:<MAILBOX_PORT>` for a Google connection. The defaults are `https://graph.microsoft.com` and `https://www.googleapis.com`.
- **A connection of the dev user** whose access token stays valid for the whole run. Whether integration-core refreshes a token against the provider's login host is not verified. The stub does not serve that host.
- **Host tools:** bash, node, curl and openssl (for the dev certificate). Git Bash on Windows works. The script converts paths with `cygpath -m` before it hands them to node.
- **Secrets stay outside the repo:** the bearer file (`EDGE_TOKEN_FILE`), the TLS key and certificate, and the recording. The recording holds the fixture code. Never commit these, and never paste a code, a token or a matched log line into chat or a ticket.
- **Sign-in and approval belong to the user.** The script never signs in, never approves and never sets a flag.

## 4. The recording

The recorded mailbox is a replay stub. From one recording file it answers integration-core's Microsoft Graph inbox read and its Gmail list and get. The same file gives the login fixture its code. One JSON file per run:

```json
{
  "fixture_code": "<the code the login fixture accepts, 4 to 10 digits>",
  "messages": [
    {
      "subject": "<subject, no digits>",
      "text": "<body; the code sits within 48 characters after an OTP keyword, e.g. 'Your verification code is <code>.'>",
      "age_seconds": 60
    }
  ]
}
```

The rules, which `--validate-recording` checks:

1. `fixture_code` is the code the login fixture accepts. The fixture reads the same file, so the two cannot disagree.
2. The code appears in the subject or the text. The first digit run in subject plus text is the code, because integration-core takes the first keyword candidate.
3. An OTP keyword sits within 48 characters before the code. The keywords are those of `otpKeyword` in `mail_otp.go`: `code`, `verification`, `login`, `sign-in`, `kode`, `bekreft`, and the rest.
4. `age_seconds` is 30 to 540. The stub re-dates every message at every request, so integration-core's default 600-second window still holds.
5. Use one message unless you have checked the order. Integration-core reads newest first and returns the first code it finds.
6. Fixture data only. Never a real person's mail.

## 5. Steps

Placeholders are in angle brackets. Use a fresh log file for every run.

0. Self-test (no network, no stack): `./scripts/evidence/otp-login-fixture.sh --self-test`
1. Validate the recording: `MAILBOX_RECORDING=<recording> ./scripts/evidence/otp-login-fixture.sh --validate-recording`
2. Start the recorded mailbox in its own terminal:
   `MAILBOX_RECORDING=<recording> MAILBOX_LOG=<fresh mailbox log> MAILBOX_BIND=<bridge address> MAILBOX_PORT=18080 ./scripts/evidence/otp-login-fixture.sh --serve-mailbox`
   Bind to the Docker bridge address, not 0.0.0.0, so the stub is not reachable from the network.
3. Make a dev certificate for the fixture host, kept outside the repo (openssl). Make the side stack's browser trust its CA. Do not turn off certificate checks.
4. Start the login fixture over https in its own terminal:
   `MAILBOX_RECORDING=<recording> FIXTURE_LOG=<fresh fixture log> FIXTURE_TLS_KEY=<key> FIXTURE_TLS_CERT=<cert> FIXTURE_BIND=<bridge address> FIXTURE_PORT=18443 ./scripts/evidence/otp-login-fixture.sh --serve-fixture`
   The run starts on `https://<fixture host>:18443/otp-login`.
5. Restart integration-core and quarry-edge on the side stack with the variables from section 3.
6. In the Verevon UI, start a browser task on that URL and say the code is in the user's mail. When the model proposes the fill, the approval card appears. Read the origin and the connection, then approve once. Never approve another origin or connection. Until B5 closes, the generic card shows the page URL and the selector only.
7. Let the task submit the form and reach the signed-in page. Note the run id.
8. Verify. This step only reads:
   ```
   EDGE_URL=<quarry-edge URL> RUN_ID=<run id> EDGE_TOKEN_FILE=<bearer file> \
   MAILBOX_RECORDING=<recording> OTP_FIXTURE_URL=https://<fixture host>:18443/otp-login \
   MAILBOX_LOG=<mailbox log> FIXTURE_LOG=<fixture log> \
   LOG_CMD_EDGE="docker logs <edge container>" LOG_CMD_IC="docker logs <integration-core container>" \
   LOG_CMD_BROKER="docker logs <broker container>" LOG_CMD_MODEL="docker logs <execution-core container>" \
   ARTIFACT_DIR=<artifact folder> RUN_ID_AMBIGUOUS=<optional run on the ambiguous page> \
   ./scripts/evidence/otp-login-fixture.sh
   ```
9. Make the checks in section 7. Then write the evidence note: the PASS and FAIL lines, the run id, the date, the stack and the commit. Never the code.

The script prints one line per check (`PASS <name>: <value>`, `FAIL ...` or `NOT-RUN ...`) and ends with `RESULT pass=N fail=N notrun=N`. NOT-RUN is not evidence. A run with NOT-RUN lines proves less: it has no mailbox log, no service logs, no artifact folder, or no ambiguous leg.

## 6. Checks

| Check | Passes when | If it fails |
|---|---|---|
| precondition: fixture URL is https; edge answers `/health` | yes; 200 | an http URL: fix the fixture. Edge down: fix the stack |
| 0 receipts readable | 200 | 404: wrong run id, or the token's user does not own the run. 401 or 403: the bearer has expired |
| 1 `fill_otp_ref` receipts on the run | exactly 1 | 0: the fill never reached the executor. Check the flags (admission answers 403 while `QUARRY_EDGE__FILL_OTP` is off), the step outcome (an `otp_*` reason is a precondition refusal), and the org (ZDR stores no receipts) |
| 2 the fill completed | 1 | the step failed: read its error code and the mailbox log |
| 3 the completed fill names its approval grant | true | false: the fill ran without a broker grant. Stop and investigate. Do not waive it |
| 4 the fill action holds ids only | true | a value, code or text key: the wire changed. Treat it as a leak |
| 5 `[redacted:otp]` in the receipts | true | false: the fill's observation was not denied (E3.1-e wiring) |
| 6, 8, 11 and every log or artifact scan | 0 | a count above 0 is a leak. Find the line yourself, with `grep -n -w -e <digits> <file>` on your own copy. Do not paste it. Find the writer. Do not publish the evidence |
| 7 inbox reads on the mailbox | at least 1 | 0: integration-core did not reach the stub. Check `MICROSOFT_GRAPH_BASE_URL` or `GOOGLE_API_BASE_URL`, the bridge address, and the connection's provider |
| 9 `POST /otp-login 303` | at least 1 | 0: the code was not accepted. Check the fixture log for `401`. Causes: the recording, the keyword or first-digit rule, or the fill went to the wrong field |
| 10 `GET /dashboard 200` | at least 1 | 0: the form was not submitted. The task needs a submit step |
| audit line in the integration-core log | pass, or NOT-RUN | NOT-RUN is normal: the audit goes to the events publisher, not to the log |
| artifacts text scan | 0 | a file matches: a stored artifact holds the code |
| 12, 13 ambiguous-page run | no completed fill; no code line | a completed fill: the two-input precondition did not refuse it |

## 7. Checks the script cannot make

1. The model transcript and the run timeline show `[redacted:otp]` at the fill, and never a digit of the code.
2. The approval card names the page origin, the frame origin and the connection (provider and label, never an address), and says that one approval covers one fill.
3. Screenshots and visual artifacts after the fill do not show the code. The observation is withheld after a fill, but a text scan cannot see pixels. Open the artifact folder and look.
4. Replay: submit the same `fill_otp_ref` again (same snapshot, ref and approval grant) on the dev page. It must be refused by the single-use set (E3.1-ab) and the spent set (E3.1-h). This re-reads the mailbox, so do it only on the dev fixture.
5. Audit: an `integration.mail.otp.read` event with outcome `found` or `not_found` is on the events stream, and it carries no code. Check the stream, because the logs do not carry it.
6. Ambiguous page: a run on `/otp-login/ambiguous` refuses the fill with a closed `otp_*` reason. Check 12 covers the completion, not the reason.

## 8. Known gaps (also in `.plan-work/quarry-w1w2/W23-depth-lane-d.md`)

- The text scan finds plain digits and a 3-and-3 split by a space or a hyphen, as whole digit runs. It misses other separators, digits split across lines, pixels, and anything the model keeps.
- ZDR runs need another evidence source, the boolean receipt that the plan names. It is not built.
- The replay probe is manual.
- The stand-in login page copies `test_site.rs` by hand. No drift check exists.
- The personal-connection decision (B2) and the publisher question (NoopPublisher) are open.
- `fill_otp_ref` names no mail connection. The connection comes from the approval binding, which is not wired (B3).

## 9. Not verified

- Nothing here has been run, syntax-checked or compiled.
- Live Graph and Gmail response shapes; integration-core's token refresh; whether the side stack's browser trusts the dev certificate; the trusted insert on `/otp-login` in Chromium; every check's real output.

Owner: the user does the sign-in, the approval and any live stack. The agent never signs in, never approves and never sets a flag.
