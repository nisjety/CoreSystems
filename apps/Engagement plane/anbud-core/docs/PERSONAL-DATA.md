# Personal data in stored notices (O1): the erasure procedure

Status (2026-10-10): the mechanism is built (migration `0006_redaction.sql`) and its database tests pass. This runbook has not been used on a real database. The legal side is not decided. Since 2026-10-28 a retention period can be applied with the same mechanism ([section 4a](#4a-retention-runs)); the period itself is not decided either, and no run has erased anything outside tests.

**This document is not legal advice.** The lawful basis named below, legitimate interest, is the owner's approved recommendation and **a recommendation only, awaiting a compliance check**. It is not a decision. Nothing here says whether a request must be granted, how long anything may be kept, or what a data subject must be told. The section [Open compliance questions](#8-open-compliance-questions) lists what is still open. This document describes how an erasure that has been decided is carried out and recorded, and what that erasure does and does not reach.

Contents: [1 Where personal data is](#1-where-personal-data-is) · [2 What the mechanism does](#2-what-the-mechanism-does) · [3 Who may run it](#3-who-may-run-it) · [4 The procedure](#4-the-procedure) · [4a Retention runs](#4a-retention-runs) · [5 After a restore](#5-after-a-restore) · [6 Rules for readers](#6-rules-for-readers-such-as-the-parser) · [7 Reference](#7-reference) · [8 Open compliance questions](#8-open-compliance-questions) · [9 Uploaded tender documents](#9-uploaded-tender-documents)

## 1. Where personal data is

Doffin notices are eForms XML. They name the buyer's contact persons with name, telephone number and e-mail address, and carry whatever free text the buyer typed. The store keeps every body byte for byte.

| Where | What it holds | Personal data | Reached by this procedure |
|---|---|---|---|
| `raw_documents.body` | The notice as downloaded. Contact persons, and any name in free text. `data_class` says `public_notice_may_hold_contacts`. | Yes: the one place the design expects it | Yes: redaction |
| `raw_search_pages.body` | Search responses: up to 100 hits per page, with buyer and winner names and free-text headings. A sole trader's name can be personal data. | Possibly | **No.** Open question 5 |
| `download_attempts` | Outcome code, HTTP status and a short message. The message is fixed text for every outcome except `http_error`, where it is the API's own error message, sanitised and clipped to 256 characters. It comes from an error response, never from a notice. | No | not needed |
| `sync_runs`, `sync_days` | State, counters and fixed text (`last_error` is fixed text only) | No | not needed |
| `raw_redactions` | The tombstones: row id, notice id, hash and size of the erased bytes, a reason code, a ticket reference, the login name of whoever ran the procedure, the time | Built to hold none. Two caveats: the login name is a person's if the login is personal, and the ticket reference shows a name if someone types one (the shape check cannot tell). | n/a |

Outside the database, and outside this procedure: server logs (the function logs ids only, but the search of step 1 puts the identifier into statement text, and a server that logs statements keeps it); backups, snapshots, WAL archives and replicas; the `psql` history of whoever searched; the ticket system; the raw responses `doffin-probe -save-dir` writes (into the directory it is given, which the project keeps at `testdata/captured/`: git-ignored, files mode 0600); Data Plane v2, where nothing is published from this core yet.

A notice that is public at the source stays public there. Erasing our copy does not remove it from Doffin.

## 2. What the mechanism does

Migration 0006 keeps `raw_documents` append-only and adds the one change an erasure needs.

- **`anbud_redact_raw_document(id, reason, request)`** is the only way to erase a body. It runs with its owner's rights (`SECURITY DEFINER`, owned by `anbud_migrator`), so the DBA group needs no privilege to change evidence. Only members of `anbud_dba` may execute it. It locks the row, refuses an unknown or already redacted id, checks the reason and the request reference, writes the tombstone, empties the body and sets `redacted_at`, all in one transaction, and returns the tombstone id. It writes one server log line with ids and the reason, and nothing from the body.
- **What a redaction changes:** `body` becomes empty and `redacted_at` is set, to the start time of the redacting transaction and not its commit time (the same time is on the tombstone). The row stays. Its id, notice id, content type, times, run and **`body_sha256`, the hash of the original bytes, do not change.** The design treats a hash of this kind as not personal data: it cannot be turned back into the text, and it keeps the identity (source, notice id, hash), so that the same bytes downloaded again find the redacted row and are not stored a second time. Open question 9 asks compliance to confirm it.
- **`raw_redactions`** has one row per redacted document and is append-only. **`raw_documents_live`** is `raw_documents` without the redacted rows, the contract for readers (section 6).
- **What cannot happen:** a body cannot be changed in any other way, a redacted row cannot be put back or deleted, a new row cannot arrive already redacted, and nobody but the function can write a tombstone. The guards are in the database, so they hold for every client. They stop mistakes and every role of the role model. The owner role `anbud_migrator` can still drop a trigger by hand, as ROLES.md says of any owner: it is a deploy-time credential, and it is not a DBA's.
- **Not reached:** `raw_search_pages` (open question 5), other versions of the same notice (a redaction names one body, not an id), backups, logs, Data Plane v2 (section 4, step 5).

## 3. Who may run it

A person the controller has named, with a **personal login role** granted into `anbud_dba`:

```text
CREATE ROLE <login> LOGIN;          -- credentials out of band, see ROLES.md
GRANT anbud_dba TO <login>;
```

- The login reaches the database through the group: `CONNECT` is granted to `anbud_dba` by name, and `roles.sql` has taken it from PUBLIC, so a role in no group cannot connect at all. The group also holds `TEMPORARY`, which is why the statements below name their schema.
- The tombstone records `session_user`, which is that login. A shared login makes the record say nothing. Behind a connection pooler that logs in as one role, it records the pooler's role.
- Not `anbud_app` (it holds no EXECUTE), not `anbud_migrator`, and not a superuser. The function checks no approval, and it cannot: **who authorises an erasure is an organisational rule.** Have the decision recorded in the ticket before step 2, and ask a second person to read the checks of step 2 before the commit if the controller wants a four-eyes rule (open question 13).
- Run it from a session that uses TLS to the database like every other client (`ANBUD_DSN` rules in REQUIREMENTS.md section 9.5).

## 4. The procedure

Use `psql -X` with your own login, and switch the history off before you start: psql decides where its history goes when it starts, so setting `HISTFILE` from inside the session is too late. `PSQL_HISTORY` is read at start:

```text
PSQL_HISTORY=/dev/null psql -X "host=<HOST> dbname=anbud_core user=<YOUR_LOGIN> sslmode=verify-full"
```

Placeholders in angle brackets are for you to fill in. A value that contains an apostrophe is written with the apostrophe doubled. Every statement below names its schema (`public.`), so that a temporary table of your own session with the same name cannot stand in for the real one.

### Step 0. Before you touch the database

1. A ticket exists. It holds who asked and how you verified them (in the ticket system, never in this database), what was asked, **the decision and who took it**, and the identifiers to search for: every spelling of the name, the e-mail address, the telephone number, and the notice ids if the requester gave any. The ticket reference is the `request` argument below: letters, digits and `. _ : / -`, up to 120 characters. It is a ticket id. Never a name, an e-mail address or anything else that identifies the requester.
2. Check that you are who you think you are, and that you may run the function:

<!-- runbook-sql: whoami -->
```sql
SELECT session_user,
       pg_has_role(session_user, 'anbud_dba', 'MEMBER') AS in_anbud_dba,
       has_function_privilege('public.anbud_redact_raw_document(bigint, text, text)', 'EXECUTE') AS may_redact;
```

   Expect your own login, `true` and `true`.
3. See whether the server logs statements. If `log_statement` is `all`, the identifier you are about to search for is written to the server log, which is another copy of it. So is it if `log_min_duration_statement` is `0`, or any threshold your find query will exceed: it reads every body, so expect it to run for minutes. And a **failing** statement is logged with its text by default (`log_min_error_statement` is `error`): a call such as `anbud_redact_raw_document(1, 'objection', 'Kari Nordmann')` is refused for its bad request reference and still writes the name to the log as `STATEMENT:`. Put nothing but the ticket reference into that call. Tell whoever runs the server what the settings are, and note it in the ticket.

<!-- runbook-sql: logging -->
```sql
SHOW log_statement;
SHOW log_min_duration_statement;
SHOW log_min_error_statement;
```

### Step 1. Find the notices that hold the identifier

This reads only. It returns ids, not bodies. Run it once for each spelling in the ticket.

<!-- runbook-sql: find -->
```sql
SELECT id, doffin_id, fetched_at
  FROM public.raw_documents
 WHERE redacted_at IS NULL
   AND position(convert_to('<IDENTIFIER>', 'UTF8') in body) > 0
 ORDER BY id;
```

- The match is on bytes and is case-sensitive. Bodies are kept exactly as downloaded and are not always valid UTF-8, which is why the body is not converted to text.
- Search the forms the XML can use: `Nordmann, Kari` and `Kari Nordmann`; the telephone number with and without spaces and country code; an e-mail address as written and in lower case. A character outside ASCII can also appear as a numeric reference such as `&#xE5;`, so search the encoded form too.
- It reads every body, so on a full corpus expect minutes of read load. Prefer a quiet time. It changes nothing, and the sync does not need to stop for it.

Then look for the identifier in the places this procedure cannot erase, and put the result in the ticket (open questions 5 and 10):

<!-- runbook-sql: find-other -->
```sql
SELECT id, query_key, page
  FROM public.raw_search_pages
 WHERE position(convert_to('<IDENTIFIER>', 'UTF8') in body) > 0
 ORDER BY id;

SELECT count(*) AS attempts_with_the_identifier
  FROM public.download_attempts
 WHERE position('<IDENTIFIER>' in message) > 0;
```

### Step 2. Redact, and check before you commit

Pick the reason that fits the ticket. The list is a set of labels, not a legal classification:

| Reason | Use it when |
|---|---|
| `erasure_request` | a data subject asked for erasure |
| `objection` | a data subject objected to the processing |
| `source_withdrawn` | the source withdrew or corrected the notice and our copy follows |
| `authority_order` | a supervisory authority or a court ordered it |
| `controller_decision` | the controller decided on its own account, for example data found that should not be held |

The erasure cannot be undone. Do it inside a transaction and read the checks before you commit.

<!-- runbook-sql: redact -->
```sql
BEGIN;

SELECT public.anbud_redact_raw_document(<ID>, '<REASON>', '<TICKET>');

SELECT t.id AS tombstone_id, t.raw_document_id, t.doffin_id, t.original_size,
       t.reason, t.request_ref, t.redacted_by, t.redacted_at
  FROM public.raw_redactions t
 WHERE t.raw_document_id = <ID>;

SELECT id, doffin_id, octet_length(body) AS body_bytes, redacted_at, left(body_sha256, 12) AS hash_start
  FROM public.raw_documents
 WHERE id = <ID>;

SELECT count(*) AS rows_in_the_live_view FROM public.raw_documents_live WHERE id = <ID>;
```

Expect one tombstone with your login and the ticket reference, `body_bytes` 0 with `redacted_at` set, and `rows_in_the_live_view` 0. The log line is written when the function runs, not when you commit, so a `ROLLBACK` leaves a log line and no tombstone. The tombstone table is the record, and the log is not. If the checks are as expected:

<!-- runbook-sql: commit -->
```sql
COMMIT;
```

If anything is off, run `ROLLBACK;` and nothing has changed. The function refuses these, and each refusal changes nothing:

| SQLSTATE | Meaning |
|---|---|
| `42501` | you are not in `anbud_dba` |
| `P0002` | no row has that id |
| `55000` | that row is already redacted |
| `22023` | the reason is not on the list, or the request reference has the wrong shape |
| `22004` | an argument was missing |
| `XX000` | the update changed no row, which only something other than this procedure can cause (a trigger that swallows updates): nothing was kept, tombstone included. Stop and tell whoever runs the database. |

For several notices in one ticket, one statement redacts them all or none:

<!-- runbook-sql: redact-many -->
```sql
SELECT n.id, public.anbud_redact_raw_document(n.id, '<REASON>', '<TICKET>') AS tombstone_id
  FROM (VALUES (<ID>), (<ID2>)) AS n(id);
```

Run the find query of step 1 again for each spelling. The ids you redacted must not come back.

### Step 3. Record it outside the database

Put in the ticket: the document ids, the tombstone ids and the notice ids; the date and your login; the output of the checks of step 2, which holds no personal data; the findings of step 1 for `raw_search_pages`, with the page ids; what remains (search pages, backups, Data Plane v2); and the controller's reply to the requester. Do not paste a body or an identifier into the ticket beyond what the ticket needs.

### Step 4. Physical removal

An `UPDATE` leaves the old row version in the table file, and its body in the TOAST table, until vacuum removes it, and a one-row change does not make autovacuum run on a large table. `anbud_dba` cannot vacuum: that needs the owner or a superuser. The owner or a superuser runs one of:

```text
VACUUM raw_documents;         -- the old version becomes invisible and its space reusable; the bytes stay on the page until the space is reused
VACUUM FULL raw_documents;    -- rewrites the table into new files, holds an exclusive lock while it does, and is the one that leaves no old version in the table files
```

Neither reaches the write-ahead log, its archive, replicas, backups, or the storage under the files. Section 5 and open question 6 are about those.

### Step 5. Derived knowledge in Data Plane v2

As of this document **nothing is published from this core to Data Plane v2.** The parser and the publisher are not built, so there is no derived record to erase there, and this step is empty today. When publication exists, the order is:

1. Redact here first (steps 1 to 3). The parser reads `raw_documents_live`, so it cannot derive from the body again.
2. Then run Data Plane v2's existing DSAR path for the derived records of that notice. Key it on the notice id and the original hash from the tombstone: both survive the redaction.
3. Record both in the same ticket.

If Data Plane v2's path is not ready when a request arrives, the redaction here does not wait for it: the open item goes into the ticket. This order is the design's recommendation and needs confirming with the owners of Data Plane v2 when publication is built.

## 4a. Retention runs

Built on 2026-10-28, for when a retention period is decided (open question 3). Until then, run it only as a dry run.

`cmd/anbud-retention` erases the bodies that have been kept a given number of months past their last use, with the function of step 2, so every erasure is the same audited change with its own tombstone. What "last use" is (`internal/retention`):

| Basis | The date it counts from |
|---|---|
| `contract_end` | The latest end of a contract the notice awards: the stated end, or the start, signing day or award day plus the stated duration. Renewals are not added. |
| `deadline` | The latest deadline for tenders or for requests to participate, of the notice or of a lot. |
| `awarded`, `issued` | The award day, or the issue time. |
| `fetched` | The day the body was fetched: the notice states none of the above, or the parser cannot read it. An unreadable body is not kept for ever. |

The latest of these dates wins, so a body is kept for the longest of its uses. A notice that awards a contract with no end is kept, however old (`open` in the output). Each body is decided on its own: the bodies of one notice's versions may expire on different days.

How it runs:

- **As a person, not a process.** It needs `ANBUD_DBA_DSN`: the DSN of your own login in `anbud_dba`, as in section 3. It refuses `anbud_app`, `anbud_migrator`, `anbud_dba` itself, any superuser and any login outside the group, before it reads anything. The tombstones record your login.
- **Dry run by default.** It reads `raw_documents_live`, and prints one JSON line for each expired body (row id, notice id, basis, the date it counts from, the expiry) and a summary line. Nothing is erased.
- **`-apply` erases.** Each body is erased in its own call of `anbud_redact_raw_document`, with reason `controller_decision` and the request reference `retention:<months>m:<day>`, which names the period and the day, never a person. A body erased by someone else since it was read is counted as `already_erased` and skipped. `-limit` (default 1000) bounds the erasures of one run; exit 2 says that more remain, and the next run goes on.
- **`-months` has no default.** The period is a decision, so every run states it. `-as-of` decides as of an earlier day, for a check; never a later one.

```text
ANBUD_DBA_DSN="host=<HOST> dbname=anbud_core user=<YOUR_LOGIN> sslmode=verify-full" go run ./cmd/anbud-retention -months <MONTHS>
ANBUD_DBA_DSN="host=<HOST> dbname=anbud_core user=<YOUR_LOGIN> sslmode=verify-full" go run ./cmd/anbud-retention -months <MONTHS> -apply
```

Exit codes: 0 done; 1 usage, configuration or a refused login, nothing erased; 2 `-limit` reached, run again; 6 a database failure, and what was erased before it stays erased. Output carries ids, dates and counts, never a body, and errors carry a SQLSTATE, never a DSN.

Step 3 still applies: record each `-apply` run in a ticket, with its summary line. Step 4 (vacuum) applies after a large run. A retention run reaches what step 2 reaches and no more: `raw_search_pages` (open question 5), backups and logs are outside it.

`make test-db` runs its database tests: a dry run erases nothing, `-apply` erases only what has expired with the tombstone above, a limited run exits 2 and the next one finishes, and every login but a DBA's own is refused with nothing erased.

## 5. After a restore

A restore from a backup, a snapshot, point-in-time recovery or a clone brings back the data as it was at the restore point. A body erased after that point is back, and a tombstone written after it is missing. `raw_redactions` cannot therefore say what has to be erased. **The authoritative list of requests is the ticket system, not this table.** Each ticket must keep what is needed to find its notices again: the notice id and the hash of the original bytes (which the tombstone also holds), and the identifiers.

After any restore, before the sync starts and before any reader reads:

1. Keep the restored database closed to the sync and to readers.
2. Take every erasure ticket from the ticket system.
3. For each notice of each ticket, find the row by its identity, the notice id and the original hash. The row id may be unknown after a restore, and the identity is not:

<!-- runbook-sql: restore-find -->
```sql
SELECT id, redacted_at IS NOT NULL AS already_redacted
  FROM public.raw_documents
 WHERE source = 'doffin' AND doffin_id = '<DOFFIN_ID>' AND body_sha256 = '<ORIGINAL_SHA256>';
```

4. Redact each row that is not redacted yet (step 2). A row that is redacted already is refused with `55000`: skip it. Where a ticket lists identifiers and no notice, run step 1 with them.
5. Check that the table and the rows agree. Both queries must return no row:

<!-- runbook-sql: restore-check -->
```sql
SELECT t.id AS tombstone_id, t.raw_document_id
  FROM public.raw_redactions t
  JOIN public.raw_documents d ON d.id = t.raw_document_id
 WHERE d.redacted_at IS NULL;

SELECT d.id
  FROM public.raw_documents d
  LEFT JOIN public.raw_redactions t ON t.raw_document_id = d.id
 WHERE d.redacted_at IS NOT NULL AND t.id IS NULL;
```

6. Note in each ticket that the erasure was applied again, and when. Tombstones lost in the restore are not rebuilt: the tickets are the history.
7. Only then reopen the database.

## 6. Rules for readers such as the parser

- Read `raw_documents_live`, never `raw_documents`. A redacted row has an empty body, and a reader of the table sees an empty document where a notice was. The view is the redaction list as a contract: it has the columns of the table, in the same order, and only the rows that still hold their body. A reader that must read the table filters on `redacted_at IS NULL`.
- The demo API (`internal/demo`, docs/ARCHITECTURE.md "Demo API") is such a reader. It reads `raw_documents_live`, parses on every request and keeps no copy, so an erasure takes effect on the next request (`TestDemoEndToEndAgainstARealDatabase` erases a body through the real function and checks it is gone from the list and the audit sheet). Its responses carry no contact person, and its log records method, path, status and duration only. What it does not cover: a profile a caller sends is a company's own business data and is not stored; if profiles are ever stored, their retention and erasure are a decision of their own.
- Anything derived from a body is outside the redaction. It is erased through Data Plane v2's DSAR path (step 5), and a parser must not keep a copy of a body.
- The parser (`eforms.Parse`) counts contact persons and never copies them. It does copy organisation names and numbers: the buyer's and, from award notices (2026-10-10), each winner's. A winner that is a sole trader (ENK) has a person's name as its name and is tied to a person by its organisation number, so a parsed winner can be personal data. See open question 16. The incumbency panel and the re-tender radar (2026-10-12) name and rank a winner only when the register confirms it is an organisation that is not a sole trader; a sole trader, and any winner that cannot be checked, is shown without name or number. The register is asked only about the winners on the page being built, and nothing is stored.
- A redaction names one body. A different body under the same notice id is another row, and the view shows it. The sync does not download an id it holds, so this happens only if another path stores one (`TestChangedBodyAfterARedactionIsStoredAsAnotherRow` pins it). See open question 7.

## 7. Reference

| Object | What it is |
|---|---|
| `raw_documents.redacted_at` | `NULL` while the body is held. Set once, by the function. |
| `raw_redactions` | Tombstones: `id`, `raw_document_id` (unique), `doffin_id`, `original_sha256`, `original_size`, `reason`, `request_ref`, `redacted_by`, `redacted_at`. Append-only. |
| `anbud_redact_raw_document(bigint, text, text)` | The only way to erase. Returns the tombstone id. |
| `raw_documents_live` | The rows of `raw_documents` that still hold their body. |
| `raw_documents_change_guard()`, `raw_documents_insert_guard()` | The triggers' functions: no `UPDATE` or `DELETE` except the function's own change, and no insert of a redacted row. |

How the function and the trigger agree: the function sets a transaction-local setting, `anbud.redacting_raw_document`, to the row id and the transaction id for the one `UPDATE`, and clears it again. The trigger admits an `UPDATE` only while that setting names this row and this transaction, the row goes from not redacted to redacted with an empty body and nothing else changed, and a tombstone for the row with its hash, its size and the same time already exists. The setting is not a secret and grants no privilege: setting it by hand opens that one change of that one row, for a role that may `UPDATE` the table, and only the owner may. A role without that privilege is refused by the server (`42501`) however it sets it.

Every table and function inside the function is named with its schema, and its search path is `pg_catalog, public, pg_temp`, with `pg_temp` last on purpose: every role may create temporary tables, and a member of `anbud_dba` who made one named like the tombstone table could otherwise have the erasure written to it, and no record kept. A test makes exactly that attempt, and shows it works against a function that is not written this way.

To rehearse the procedure, `make test-db` runs the SQL blocks of this document on a throwaway database (`TestRunbookSQLRuns`) and the mechanism's own tests. Only PostgreSQL 16.14 has been tried.

## 8. Open compliance questions

None of these is decided, and none is built. Each needs an answer from compliance or the owner.

1. **Lawful basis and the balancing test.** The recommended basis is legitimate interest: professional contact data that authorities publish under procurement law, processed for tender discovery. It needs a written assessment. Does it hold for free text, for names that are not contact fields, and for anything of a special kind a buyer may have typed? The store keeps whatever the notice said.
2. **Transparency to data subjects whose data came from a public register.** Whether and how they are told, whether an exemption applies, and where the notice is published.
3. **Retention.** The recommendation is no automatic expiry while the source keeps the notice public. The sync never checks that again: it downloads an id once and never re-reads a held one, so a notice withdrawn or corrected at the source stays here. Is that acceptable, or does retention need a period, a re-check, or a rule per purpose? *Built, not decided (2026-10-28):* if a period is chosen, section 4a applies it; the engineering recommendation is a period after the last use, not no expiry, because storage limitation asks for one. Which period, and whether it differs per purpose, is still open.
4. **Response times.** How the steps above fit the deadline for answering a request, who triages, and what the requester is told about backups and logs.
5. **Names in search pages.** `raw_search_pages` bodies hold buyer and winner names (a sole trader's name can be personal data) and free-text headings. A page holds up to 100 notices, and blanking a page destroys the evidence that a day was complete. **Per-hit redaction would need new code and is not built.** The options are to accept the pages under the same basis, to build per-hit redaction, to stop keeping page bodies once a day is complete and keep counts and hashes, or to blank whole pages and mark their days as having lost their evidence. Until this is decided the procedure finds identifiers in pages (step 1) and erases none.
6. **Backups and everything that copies the database.** A restore brings back what was erased, and the tombstones with it if the backup is older. How long backups, WAL archives and replicas live, whether that is acceptable for erased data, and whether exports, test copies or analyst dumps exist.
7. **Standing requests.** A request is about a person, and new notices that name the person will arrive. The tombstone is not a block list: it names one body, not a person or an id. A suppression list that matches identifiers when a notice is stored or parsed would need a basis of its own, because it keeps the identifier, and code that does not exist. Or each arrival is handled as it comes.
8. **Who the controller is.** Which legal entity operates the shared corpus and so decides its purposes and means, whether tenants are separate controllers for the copies they hold, who answers requests, and who the DBAs work for.
9. **What the tombstone keeps.** The notice id and the hash of the erased bytes (treated here as not personal data), the DBA's login name (staff data, unless logins are named by role), and the ticket reference. And the ticket system is another store of the requester's data, with its own retention.
10. **Logs and tools that copy an identifier.** Statement logs, monitoring, `pg_stat_activity`, `psql` history and ticket attachments. Whether `log_statement` may be on for this database.
11. **Records and assessments.** The entry in the records of processing for the corpus, the tombstones and this procedure, and whether the processing needs an impact assessment.
12. **Where the data is and who can reach it.** The hosting of the database, any transfer out of the EU, and who holds the owner credential.
13. **Approval.** Who authorises an erasure, and whether a second person must check it. The function checks no approval.
14. **Scope of one request.** Whether a request covers every version of a notice and every notice that names the person, or only the notices the requester points to.
15. **The tombstones as a record about people.** A tombstone keeps the notice id, the reason and the DBA's login for as long as the table exists, and with the notice id it can show that someone named in that notice exercised a right (`erasure_request`, `objection`). How long tombstones may be kept, who may read them, whether a requester who asks what is held about them is shown one, and whether the find query of step 1 must be audited: no record is kept of who searched for which name, and the only trace is the server log, if the server logs statements (question 10).
16. **Winners who are sole traders.** Award notices name winners, and the parser keeps each winner's name and organisation number for the incumbency panel and win history. For a sole trader both point at a person. Whether that is covered by the same basis as buyer contacts, whether sole-trader winners should be shown only in aggregate (the council's advice), whether this needs an impact assessment, and how a sole trader's erasure request reaches parsed and published winners: undecided. Nothing is published yet. Built as a proposal (2026-10-12): the demo names and ranks only winners the register confirms are not sole traders, and withholds the rest; compliance should confirm or change it.
17. **Uploaded tender documents hold other people's data.** A package of tender documents names contact persons, and a tender's annexes hold CVs and references of staff. Who is the controller of what a company uploads and who the processor, whether a processing agreement with the tenant comes before the feature is switched on for one, and what the tenant is told. Section 9 says what this core keeps; it does not answer this.
18. **Retention of uploads.** The default of 90 days (`ANBUD_TENDER_DOCS_RETENTION`, at most two years) is a placeholder, not a decision: how long a tenant's uploaded documents may be kept, and whether the tenant or the operator decides.
19. **Erasure of a whole organisation's uploads.** There is no hook from org-core: when a tenant leaves, a DBA deletes its rows by hand (section 9). Whether that is acceptable, and who triggers it.
20. **Names in quoted sentences.** A finding quotes the sentence of a document that names a certificate, and a sentence can name a person ("the contact person X shall ..."). Whether names must be removed from the sentences before they are kept, and whether the archive's own name, which the uploader types, may be kept.

## 9. Uploaded tender documents

Status (2026-10-10): built, opt-in (`ANBUD_TENDER_DOCS`), tested against PostgreSQL 16.14. Not used on a real tenant. The design is in [ARCHITECTURE.md, Tender documents](ARCHITECTURE.md#tender-documents); this section is what a DBA needs for personal data.

These are **not** evidence and are outside everything above. They are a tenant's own documents, in schema `tenant`, and the erasure mechanism of sections 2 to 5 (redaction, tombstones, `raw_documents_live`) does not apply to them: an upload is **deleted**, not redacted, and no tombstone is written.

| Where | What it holds | Personal data | Reached by |
|---|---|---|---|
| `tenant.tender_uploads.documents` | The manifest: for each file its path in the archive, kind, size, SHA-256 and whether it was read. | A path can carry a name (`CV Ola Nordmann.pdf`). | Delete the upload |
| `tenant.tender_uploads.findings` | The sentences of the documents that name a certificate or registration the rules know, with file and page or paragraph, at most 50 of each kind and 400 in all. | A sentence can name a person. Open question 20. | Delete the upload |
| `tenant.tender_uploads.archive_name`, `uploaded_by` | The name the uploader gave the archive, and the user id the gateway sent. | The name is typed by a person. The id is an id, never a name. | Delete the upload |
| The text and the bytes of the documents | Not kept: read while the request is open, in memory, and dropped. | Yes, while the request lasts. | Nothing to erase |
| The server log | Counts for each upload (files, read, unread, findings, time). Never a file name, a sentence, an organisation or a user. | No | n/a |

Not reached: backups, snapshots and WAL archives of the database (section 5 applies in the same way: a restore brings back an upload that was deleted); a logical dump (`pg_dump`) or an analyst's copy of the database, which holds schema `tenant` unless it is excluded (`--exclude-schema=tenant`), so a copy made for another purpose can hold a tenant's sentences (question 6); the process memory of a request while it lasts, and of the PDF child process, which exits after each file.

**Retention and deletion.** An upload expires after `ANBUD_TENDER_DOCS_RETENTION` (default 90 days, at most two years, which the table enforces: it refuses an expiry more than two years and two days after the row was made). From that moment no read sees it, and an hourly purge (`tenant.purge_expired_uploads()`) removes the row. The organisation can delete an upload at once (`DELETE /api/v1/notices/{id}/documents/{upload_id}`), and uploading the same archive again is one upload and does not duplicate it. An organisation in zero-data-retention mode cannot upload: the request is refused with 409 before anything is read.

**Erasing one organisation's uploads (an operator's procedure; there is no hook from org-core).** Run as the owner (`anbud_migrator`) or a superuser, deliberately: row level security hides every row from a session that has not named an organisation, and the DBA group and `anbud_app` hold nothing that reads another organisation's rows by design.

```sql
-- Count first, then delete in one transaction and check the number.
BEGIN;
SELECT count(*) FROM tenant.tender_uploads WHERE org_id = :'org';
DELETE FROM tenant.tender_uploads WHERE org_id = :'org';
-- COMMIT only if the number deleted is the number counted.
```

Record the request outside the database, as step 3 does. The rows are not evidence and no tombstone is kept, so the ticket is the whole record.
