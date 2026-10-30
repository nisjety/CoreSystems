# Event Lead Registration Script — Full Summary

This document covers the SuperOffice Customer Centre lead-capture script (`event_lead_registration.crmscript`) and the related one-off backfill script (`backfill_project_members_5563.crmscript`). It explains what the scripts are for, how they work internally, what was fixed/improved in this workstream, and how to deploy, access, and keep maintaining them.

Files referenced:
- [scripts/event_lead_registration.crmscript](../scripts/event_lead_registration.crmscript) — the live public-facing lead form
- [scripts/backfill_project_members_5563.crmscript](../scripts/backfill_project_members_5563.crmscript) — one-time data-repair script
- [config/Swagger-v1-Agents.json](../config/Swagger-v1-Agents.json) / [config/Swagger-v1-REST.json](../config/Swagger-v1-REST.json) — local API reference used to verify field/entity behavior

---

## 1. What this script is used for

`event_lead_registration.crmscript` is a **SuperOffice CRMScript** that runs directly inside SuperOffice's Customer Centre engine (`customer.fcgi?action=safeParse`). It is *not* part of the Python `socrm_service` app — it's a standalone script uploaded to and executed by SuperOffice itself, used as a public/embeddable **event lead capture form** (currently for **Havexpo 2026**, project id `5386`).

When someone fills out the form (e.g. from a booth tablet, QR code landing page, or embedded iframe at a trade show), the script:

1. Renders an HTML form (company, name, email, phone, country, position, what the lead wants, sale owner, sale type, sale stage, etc.).
2. On submit (`POST`), validates the input.
3. Creates (or reuses) a **Company** (`NSContactEntity`), a **Person** (`NSPersonEntity`), and a **Sale** (`NSSaleEntity`) in SuperOffice.
4. Links the person to the shared event **Project** (`5386` / "CO Messe Havexpo 2026") as a project member.
5. Optionally adds the person to a **Selection** (marketing list) if a lead selection is configured.
6. Shows a success page, or rolls back everything it created if any step fails.

`backfill_project_members_5563.crmscript` was a **one-time repair script** used to fix historical data: it found people who were members of a marketing *selection* (id `3862`, Seafood Expo) but were missing from the corresponding *project* (id `5563`), and added them as project members.

---

## 2. What we did in this chat (chronological)

### A. Fixed the backfill script (`backfill_project_members_5563.crmscript`)
- **Bug**: `GetSelectionMembers` is not a valid CRMScript API call (SuperOffice suggested `AddSelectionMembers` instead, which was also wrong for reading data).
- **Fix**: Replaced it with a proper `SearchEngine` query against the `selectionmember` table:
  ```
  SearchEngine se;
  se.addFields("selectionmember", "person_id,contact_id");
  se.addCriteria("selectionmember.selection_id", "OperatorEquals", LEAD_SELECTION_ID.toString());
  ```
- **Bug**: initial criteria used the wrong column name (`selectionId` instead of `selection_id`), which threw `SearchEngine::convertPreSevenSyntax() selectionId not found!`.
- **Bug**: initial loop used `execute()` and numeric `getField(0)` — non-standard for this codebase. Standardized on `select() / first() / next()` plus named `getField("table.field")`, matching patterns used elsewhere in the repo.
- **Data-quality issue discovered at runtime**: many selection-member rows had `contact_id` blank/0, even though `person_id` was valid — meaning the raw selection data alone wasn't enough to build a project member (project membership needs both person and company/contact ids).
- **Fix**: added a fallback resolution path — when `contact_id` is missing, look up the person and pull their linked company:
  ```
  NSPersonEntity person = personAgent.GetPersonEntity(personId);
  NSContact contact = person.GetContact();
  resolvedContactId = contact.GetContactId();
  ```
  Results were cached in a `Map` (`resolvedContactIds`) so the same `personId` is never looked up twice.
- **Result (verified by live run)**:
  - Existing project members: 29
  - Selection members found: 59
  - To add: 30 | Already in project: 29 | Resolved missing contact IDs: 30 | Contact lookup failures: 0 | Invalid rows: 0
  - Batch 0-30: OK
  - **Done. Added: 30**

This script is safe to re-run (it skips anyone already in the project) but it's a one-off tool tied to specific project/selection IDs — it's not meant to run on a schedule.

### B. Hardened the event registration script (`event_lead_registration.crmscript`)
- **Reviewed field semantics** to make sure project membership vs. selection membership use the right identifiers:
  - `NSProjectMember` correctly requires **both** `PersonId` and `ContactId` (person + their company).
  - `NSContactPersonId` (used for selection membership) only supports **either** `PersonId` or `ContactId`, not both — so selection membership in this script intentionally stays **person-based**.
- **Fixed a silent failure**: previously, if `AddProjectMembers` failed, the error was swallowed and the lead/person/company were still left behind with no project link (this is exactly what caused the need for the backfill script above). The failure is no longer swallowed.
- **Added full rollback / transactional behavior**: if anything fails after a company/person/sale/project-member/selection-member has already been created, the `catch` block now un-does everything it created, in reverse order:
  1. Remove selection membership (`RemoveContactSelectionMembers`) — only if it was added by this run.
  2. Remove the project member — looked up via `GetProjectMembers(newProjectId)` and matched by `PersonId`, deleted via `DeleteProjectMember(cleanupProjectMember.GetProjectmemberId())`.
  3. Delete the sale (`DeleteSaleEntity`) — only if this run created it.
  4. Delete the person (`DeletePersonEntity`) — only if this run created it.
  5. Delete the company (`DeleteContactEntity`) — only if this run created it.
  - Each "only if this run created it" is tracked with boolean flags: `createdCompany`, `createdPerson`, `createdSale`, `projectMemberAdded`, `selectionMemberAdded`. This avoids ever deleting pre-existing data that the script *reused* (e.g. an existing company matched by name, or an existing person matched by email/phone).

### C. Retargeted the script from Seafood Expo → Havexpo 2026
- Old constants (Seafood Expo Global 2026, project `5563`) were swapped for:
  ```
  String EVENT_NAME = "Havexpo 2026";
  Integer SHARED_PROJECT_ID = 5386;
  String SHARED_PROJECT_NAME = "CO Messe Havexpo 2026";
  ```
- First attempt tried to resolve the project dynamically by name; when the user reported "Unable to find the event lead selection in SuperOffice" and shared a project URL showing `project_id=5386`, the script was simplified to use that **explicit project id** directly (`newProjectId = SHARED_PROJECT_ID;`) instead of a fragile name lookup.
- **Lead selection made optional/non-blocking**: `LEAD_SELECTION_ID` is currently `0` and `LEAD_SELECTION_NAME` is empty, so `leadSelectionResolved` evaluates to `false` and the script simply skips adding selection membership rather than failing the whole submission. This was a deliberate decision so the Havexpo form works end-to-end even though the correct Havexpo marketing selection hasn't been confirmed yet (see [Section 6 — Open Items](#6-open-itemsknown-gaps)).

### D. Fixed phone number sync
- **Bug**: the person-save block only wrote `person.SetOfficePhones(phoneArr)`. Downstream code (e.g. [sync/bigquery_sync.py](../sync/bigquery_sync.py)) and typical SuperOffice UI/reporting expect a **mobile** number to be present too (`MobilePhones`), since leads submitted from a form are almost always mobile numbers.
- **Fix**: now writes the submitted `leadPhone` into **both** collections:
  ```
  NSEntityElement[] mobilePhoneArr;
  NSEntityElement mobilePhoneEntry;
  mobilePhoneEntry.SetValue(normalizedLeadPhone);
  mobilePhoneArr.pushFront(mobilePhoneEntry);
  person.SetMobilePhones(mobilePhoneArr);

  NSEntityElement[] officePhoneArr;
  NSEntityElement officePhoneEntry;
  officePhoneEntry.SetValue(normalizedLeadPhone);
  officePhoneArr.pushFront(officePhoneEntry);
  person.SetOfficePhones(officePhoneArr);
  ```
- Verified with `get_errors` (no syntax errors) and a `grep` confirming both setters are present. **Still needs a live test** in SuperOffice after publishing (local edits don't auto-publish — see Section 4).

---

## 3. How the script works (step-by-step architecture)

The whole thing lives in a single file between `%EJSCRIPT_START%` and `%EJSCRIPT_END%` markers (SuperOffice's script wrapper) — there's no separate template/controller split; HTML and CRMScript logic are interleaved with `<% %>` tags, similar to PHP/JSP.

### 3.1 Constants & setup (top of file)
```
String EVENT_NAME = "Havexpo 2026";
Integer SHARED_PROJECT_ID = 5386;
String SHARED_PROJECT_NAME = "CO Messe Havexpo 2026";
Integer SALE_AMOUNT = 0;
Integer SALE_CLOSE_DAYS = 30;
Integer LEAD_SELECTION_ID = 0;      // 0 = no selection membership added
String LEAD_SELECTION_NAME = "";
String SCRIPT_KEY = "KyQ8bwBfzXOiwda8";
```
It also builds the script's own callback URL:
```
String scriptUrl = getParserVariable("AuthProgram") + "?action=safeParse&includeId=eventLeadRegistration&key=" + SCRIPT_KEY;
```
This is the same URL pattern the form action/self-link uses, and the same pattern you use to access the script from outside (see Section 4).

### 3.2 Reading form input
All fields come from `getCgiVariable(...)` (GET/POST parameters): `leadName`, `leadEmail`, `leadPhone`, `orderName`, `companyName`, `contactTitle`, `contactPositionId`, `contactCountry`, `businessCategoryId`, `companyOwnerAssocId`, `leadOwnerAssocId`, `saleTypeId`, `customerCategoryId`, `saleStageId`, `whatLeadWants`. Everything is normalized (`stripLeadingAndTrailing`, `.toLower()` for email) before use.

### 3.3 Validation
Required-field checks run only on POST (`isPosted`): company name, lead name, valid email, lead owner selected, sale type selected, resolvable "Prospect" customer category, and a non-empty "what does the lead want" field. Country (if provided) is matched against `NSListAgent.GetCountries()` by name / English name / ISO2 / ISO3.

### 3.4 Duplicate detection (reuse instead of duplicate)
Before creating anything, the script tries to find existing records so re-submissions and known contacts don't create duplicates:
- By email → `Customer.findFromEmail(...)`
- By phone → `Customer.findFromCellphone(...)`
- By company name → `SearchEngine` against `contact.name`

If found, `resolvedPersonId` / `resolvedCompanyId` are set and the corresponding `reusedExistingPerson` / `reusedExistingCompany` flags are `true` — these existing records are **never deleted** during rollback.

### 3.5 Creating the Company
`NSContactAgent.CreateDefaultContactEntity()` → set name, associate/owner, business category, customer category, country → `SaveContactEntity`. Skipped entirely if an existing company was resolved above.

### 3.6 Creating the Person
`NSPersonAgent.CreateDefaultPersonEntity()` → set first/last name, title, position, link to company via `NSContact`, country, email (`SetEmails`), **and now both** `SetMobilePhones` **and** `SetOfficePhones` → `SavePersonEntity`. Skipped if an existing person was resolved above.

### 3.7 Creating the Sale
- `NSSaleAgent.CreateDefaultSaleEntity()`.
- The **project is fixed**: `newProjectId = SHARED_PROJECT_ID;` (no more dynamic name lookup).
- Lead selection: uses `LEAD_SELECTION_ID` if set (>0); otherwise tries to resolve `LEAD_SELECTION_NAME` via `SearchEngine`; otherwise stays unresolved (`leadSelectionResolved = false`) and is simply skipped later — **it never blocks lead creation**.
- Duplicate-sale check: if a sale already exists for this project/person/heading combo, it's reused (`reusedExistingSale`) instead of creating a second one.
- Sale stage/rating is resolved from the chosen `saleTypeId`'s available stages (`NSSaleTypeEntity.GetStages()`), matching the submitted `saleStageId`, with a fallback to the first available stage if no match.
- If the shared project can't be resolved, or no valid sale stage can be assigned, the script rolls back anything already created (person/company) and shows a form error instead of a generic crash.
- Otherwise: sets sale text, contact, person, associate (owner), amount, close date (`SALE_CLOSE_DAYS` days out) → `SaveSaleEntity`.

### 3.8 Project membership
Checks `GetProjectMembers(newProjectId)` for an existing row with this `PersonId`; if none exists, builds an `NSProjectMember` with **both** `PersonId` and `ContactId` and calls `AddProjectMembers`. Sets `projectMemberAdded = true` so rollback knows to clean it up on failure.

### 3.9 Selection membership (optional)
Only runs if `leadSelectionResolved` is `true`. Checks for an existing `selectionmember` row (via `SearchEngine`) for this person+selection; if none, adds one via `NSContactPersonId` (person-based only) and `AddContactSelectionMembers`. Sets `selectionMemberAdded = true` for rollback purposes.

### 3.10 Success / error handling
- On success: `showSuccess = true`, builds a user-facing success message (different wording depending on whether an existing sale/person/company was reused).
- On any exception in the whole creation block: the `catch` runs the full reverse-order rollback described in Section 2B, then sets a generic `formError`.
- The rest of the file (below line ~560) renders the actual HTML page: dropdown option lists (owners, sale types, customer categories, sale stages, countries, business categories, positions) sourced from `NSMDOAgent` / `NSListAgent`, then the `<!DOCTYPE html>` form itself, with `<% if (!formError.isEmpty()) { %>` blocks for inline error display and a distinct success view when `showSuccess` is true.

---

## 4. Deploying & accessing the script in SuperOffice

### Where the script actually lives
This `.crmscript` file in the repo is the **source of truth for review/version history**, but it is **not automatically deployed**. SuperOffice Customer Centre scripts must be uploaded/pasted into SuperOffice's own script management UI (Admin → Customer Center Scripts, or via the SuperOffice developer tools depending on your tenant setup) under the script id **`eventLeadRegistration`**. After editing this file locally, you must manually copy/publish the updated script content into SuperOffice before changes take effect live.

### Accessing/embedding the deployed script
Once published, the script is invoked through SuperOffice's `safeParse` action. The confirmed live URL for this tenant is:
```
https://online3.superoffice.com/Cust29215/CS/scripts/customer.fcgi?action=safeParse&includeId=eventLeadRegistration&key=KyQ8bwBfzXOiwda8
```
Generic pattern:
```
<AuthProgram>?action=safeParse&includeId=eventLeadRegistration&key=KyQ8bwBfzXOiwda8
```
Where:
- `<AuthProgram>` resolves to `https://online3.superoffice.com/Cust29215/CS/scripts/customer.fcgi` for this tenant (`online3.superoffice.com` = SuperOffice pod, `Cust29215` = this customer's tenant id).
- `action=safeParse` tells SuperOffice to run a Customer Centre script.
- `includeId=eventLeadRegistration` identifies *which* script to run (must match the id the script was published under in SuperOffice).
- `key=KyQ8bwBfzXOiwda8` is the script's access key (`SCRIPT_KEY` constant at the top of the file) — required by SuperOffice to authorize running this specific safeParse script.
- An optional `&withFrame=1` can be appended to render the form inside SuperOffice's own Customer Centre chrome/navbar (used while comparing against the working `formFrame` version during debugging); omit it for a bare embeddable page (e.g. in an iframe).

This full URL is what you'd put behind a QR code, an iframe `src`, or a link on a landing page for the trade show booth. If you regenerate/rotate `SCRIPT_KEY`, update it in **both** the script constant and anywhere the URL is shared/embedded.

### After deploying — how to verify it's working
1. Open the URL above in a browser — you should see the Havexpo 2026 lead form render (owners/sale types/etc. populated from live SuperOffice lists).
2. Submit a test lead with a phone number and check in SuperOffice:
   - A new/updated **Company** and **Person** exist with the expected name/email.
   - The **Person**'s Mobile Phone *and* Office Phone fields both show the submitted number.
   - A **Sale** exists under project `5386` ("CO Messe Havexpo 2026") linked to that person/company.
   - The person shows up under **Project → 5386 → Members**.
3. Submit again with the same email/phone to confirm duplicate detection reuses the existing company/person instead of creating new ones, and reuses the sale instead of duplicating it.
4. To test rollback, you can temporarily force an error (e.g. invalid sale type id) after project/selection membership would have been added, and confirm no orphaned company/person/sale/project-member is left behind.

---

## 5. How to keep using & improving it

- **Treat this repo file as the reviewable source**: make edits here first (with the benefit of `get_errors`/lint checks and version history), then copy the final content into SuperOffice's script editor to publish. Keep a habit of pasting the exact published content back here afterward if SuperOffice-side hotfixes are ever made directly in the admin UI, so the two never drift.
- **Constants block is the main "config" surface** — for a new event/project, you should only need to change:
  ```
  EVENT_NAME, SHARED_PROJECT_ID, SHARED_PROJECT_NAME, LEAD_SELECTION_ID / LEAD_SELECTION_NAME
  ```
  Avoid hardcoding new IDs deeper in the script body — keep them centralized at the top like today.
- **Validate with `get_errors`** on the file after any edit (CRMScript syntax errors are caught statically) before publishing to SuperOffice.
- **Keep the rollback flags pattern** (`createdX` / `reusedExistingX` / `xAdded`) whenever adding a new creation step (e.g. if you ever add a document, activity, or additional selection) — every new "create" needs a matching "undo" in the `catch` block, and must not delete anything that was *reused* rather than created.
- **Selection/project semantics to remember**:
  - Project membership (`NSProjectMember`) needs **both** `PersonId` and `ContactId`.
  - Selection membership (`NSContactPersonId`) accepts **either** `PersonId` or `ContactId`, not both — this script deliberately stays person-based for selections.
  - `selectionmember.contact_id` is **not reliable** — don't assume it's populated; resolve via the person's linked company (`GetContact().GetContactId()`) when needed, and cache lookups per person id to avoid repeat calls.
- **Testing changes safely**: since this script creates real CRM data, test with clearly fake company/person names and clean them up afterward (or reuse the rollback-by-error trick above) rather than testing directly against production-looking leads.
- **Local Swagger files are the fastest way to check field/entity availability** without needing live API access — search [config/Swagger-v1-Agents.json](../config/Swagger-v1-Agents.json) and [config/Swagger-v1-REST.json](../config/Swagger-v1-REST.json) for entity names like `PersonEntity`, `ProjectMember`, `ContactPersonId` before assuming a setter/getter exists.
- **The backfill script is a template**, not a scheduled job — if a future event similarly ends up with selection members missing from a project (e.g. due to a bug like the one fixed in Section 2B), copy `backfill_project_members_5563.crmscript`, update `PROJECT_ID` / `LEAD_SELECTION_ID`, and re-run once. It's idempotent (safe to re-run, skips existing members) but isn't meant to run continuously.

---

## 6. Open items / known gaps

- **Havexpo lead selection is not yet confirmed.** `LEAD_SELECTION_ID = 0` and `LEAD_SELECTION_NAME = ""`, so selection membership is currently **skipped entirely** for Havexpo leads. If Havexpo needs leads added to a marketing selection/list (like Seafood Expo did with selection `3862`), find the correct selection name/id in SuperOffice and set one of those two constants.
- **Phone sync fix needs a live test.** The mobile+office phone change was only validated statically (`get_errors` passed); it hasn't been confirmed against a real SuperOffice submission yet since the script must be re-published first.
- **Orphaned test file from an abandoned architecture.** [tests/test_public_lead_registration_lookups.py](../tests/test_public_lead_registration_lookups.py) still patches `app_module._create_public_lookup_client` and calls `/api/public/lead-registration/lookups` — neither exists in [app.py](../app.py) anymore (see Section 7 below). This test is dead and will fail/error if it's ever run; it should be deleted or rewritten once someone confirms it's no longer needed.

---

## 7. Architecture history — how dropdown data loading evolved (and a security fix along the way)

Earlier in this project's history, before the script settled on its current design, the dropdown data (lead owners, sale types, etc.) went through two abandoned approaches worth knowing about, since remnants of the second one still exist in the repo:

1. **Direct browser-side calls to SuperOffice's own WebAPI.** The form's JavaScript tried to `fetch()` `/Cust29215/api/v1/User` and `/Cust29215/api/v1/List/SaleType/Items` directly from the visitor's browser. This failed with `401 Unauthorized` — the public Customer Centre page has no way to authenticate a browser-side call against SuperOffice's WebAPI.
2. **A backend "lookup proxy" in this Flask app.** The script was changed to fetch a single lookups payload from this `socrm_service` app instead, at a new route `/api/public/lead-registration/lookups`, backed by system-user auth in [clients/socrm_system_user_client.py](../clients/socrm_system_user_client.py), with CORS opened for `https://online3.superoffice.com`. The lookup base URL was made overridable via a `lookupBaseUrl` query-string parameter (originally intended for local testing against `http://localhost:5005`).
   - **Security issue found in review**: allowing `lookupBaseUrl` to be set from an untrusted query parameter meant anyone could craft a link like `...&lookupBaseUrl=https://evil.com` and silently redirect the lookup fetch to an attacker-controlled server — a URL-injection/SSRF-style vulnerability with phishing/data-exfiltration potential. This was rated **CRITICAL** and fixed by removing the query-string override entirely, restricting CORS to non-credentialed requests from the SuperOffice origin only, and adding a short-lived cache (5 min TTL), a simple rate limit (30 requests/minute), and bounded pagination when paging through associates.
   - This required the Flask app to be **running and publicly reachable** for the live form to load its dropdowns at all — which caused a real outage in testing (`GET http://localhost:5005/api/public/lead-registration/lookups net::ERR_CONNECTION_REFUSED`) because nothing was actually deployed/listening at that lookup URL from the public internet.
3. **Final decision: drop the backend dependency entirely.** To remove that fragile external dependency (and the security surface that came with it), the script was rewritten from scratch to populate all dropdowns using **native CRMScript calls only** — `NSMDOAgent.GetSimpleList(...)` with a `SearchEngine` fallback (see Section 3.10's dropdown-loading description) — so the published script is now fully self-contained and does not call out to this Flask app at all.

**Takeaway for future work**: the current `event_lead_registration.crmscript` has **no runtime dependency on `socrm_service` being deployed anywhere** — it only talks to SuperOffice's own CRMScript APIs. Don't reintroduce an external lookup dependency without repeating the CORS/URL-allowlisting hardening above, and clean up the orphaned test noted in Section 6 so the history doesn't cause confusion later.
