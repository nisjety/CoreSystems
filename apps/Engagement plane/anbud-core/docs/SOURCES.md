# Sources

What each notice source actually exposes. Everything marked **verified** was observed on the live provider on 2026-10-09 with the project's own keys (`cmd/doffin-probe`, a 22-notice corpus sample, and anonymous TED queries), or on 2026-10-10 in the live smoke test (see [the last section](#live-smoke-test-2026-10-10)). Anything else says so.

## Doffin Public API (primary feed)

- Base: `https://api.doffin.no/public/v2`
- Auth: header `Ocp-Apim-Subscription-Key`. Keys are free, via the API Management portal. Production and test are separate systems.
- **Verified:** both the Primary and the Secondary key of the subscription are accepted, and they share one budget.
- One subscription is **one global budget**, whichever key is used.

| Operation | Limit (per subscription) | Notes |
|---|---|---|
| `GET /search` | **30 requests/min** | Returns a page of notice summaries. |
| `GET /download/{doffinId}` | **10 requests/min** | `doffinId` is a **path** parameter. The id looks like `2026-115718`. |

A 429 carries `Retry-After`. 15 searches and 22 downloads, paced at 24 and 8 per minute, produced no 429. Nor did the 50 searches and 2 downloads of the smoke test on 2026-10-10.

### `search` parameters

| Parameter | Type | Verified behaviour |
|---|---|---|
| `numHitsPerPage` | integer | Maximum 100 (a 100-hit page is about 245 KiB of JSON). |
| `page` | integer | **1-based.** `page=0` is rejected with HTTP 400. **Page boundaries are not stable under tied sort keys:** on a multi-page day a later request can return the tied hits in another order, so a sweep over the pages can repeat ids and miss others (V2, 2026-10-10). A day that fits on one page is not affected. |
| `sortBy` | string | Default `PUBLICATION_DATE_DESC`. **Accepted:** `PUBLICATION_DATE_DESC`, `PUBLICATION_DATE_ASC`, `DEADLINE`, `RELEVANCE`, `ESTIMATED_VALUE_DESC`. **`ISSUE_DATE_ASC` and `ISSUE_DATE_DESC` (listed in the OpenAPI enum) return HTTP 404.** The description text was right and the OpenAPI enum wrong. A bad sort value gives 404, not 400. |
| `searchString` | string | Free text. |
| `type` | string, repeatable | Matches a notice's concrete type **or its category** (see below). |
| `status` | string, repeatable | `ACTIVE`, `EXPIRED`, `AWARDED`, `CANCELLED`. Only competition-type notices carry a status. |
| `cpvCode` | string, repeatable | |
| `location` | string, repeatable | NUTS-style code such as `NO081`; `anyw` means "not specified". |
| `issueDateFrom`, `issueDateTo` | `yyyy-mm-dd` | The tool for slicing (see "Volumes"). **Verified 2026-10-10:** both bounds include their day, and the filter selects by the hit's `publicationDate`, not by its `issueDate` timestamp, which can be a day earlier (V1; see [the smoke test](#live-smoke-test-2026-10-10)). |
| `estimatedValueFrom`, `estimatedValueTo` | long | |

**Types form a hierarchy.** Each hit's `allTypes` lists its concrete type followed by its category: `COMPETITION` covers `ANNOUNCEMENT_OF_COMPETITION`, `DYNAMIC_PURCHASING_SCHEME` and `QUALIFICATION_SCHEME`; `RESULT` covers `ANNOUNCEMENT_OF_INTENT`, `ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT`, `CHANGE_OF_CONCLUSION_OF_CONTRACT` and `CANCELLED_OR_MISSING_CONCLUSION_OF_CONTRACT`; `PLANNING` covers the advisory, pre-announcement and buyer-profile types. Active `COMPETITION` notices numbered 1066 against 622 for `ANNOUNCEMENT_OF_COMPETITION` alone. A filter also matches against `allTypes`, so `type=NOTICE_ON_BUYER_PROFILE` returned a notice whose own type is `ANNOUNCEMENT_OF_COMPETITION`.

### `search` response (verified over 100 notices)

`{ numHitsTotal, numHitsAccessible, hits[] }`. **`hits` is a list**, although the OpenAPI types it as an object.

- **`numHitsAccessible` is capped at 1000** while `numHitsTotal` was 159 833. A query can never reach past its first 1000 hits, so a complete sync must slice by date window.
- `status` is a string only on competition notices and `null` otherwise. `deadline` is a timestamp or `null`.
- `issueDate` is a full timestamp (`2026-10-07T13:32:11Z`); `publicationDate` is a plain date.
- `estimatedValue` is `null` on 53 of 100 notices; when present `amount` is a JSON float.
- `receivedTenders` and `allReceivedTenders` are `null` except on result notices (codes such as `tenders`, `t-esubm`).
- `buyer[].organizationId` is **not normalised** (`957 387 969` with spaces). 94 of 100 notices have one buyer, the rest up to nine.
- `lots[]` carries only `heading`, `description` and `winner[]`; winners appear only on result notices.
- `limitedDataFlag` and `doffinClassicUrl` were `null` on every notice.

### `download`

- Returns the **eForms UBL XML** of the notice, 8 KB to 540 KB in the sample (a qualification system with about 100 entities).
- **`Content-Type` is `application/octet-stream`** whatever it holds, so the format must be sniffed from the body, never trusted from the header.
- The Doffin identifier is **not** in the XML (DFØ: it is not part of the payload), so it must be kept from the request.
- Even the oldest notice (published 2017-01-02) downloads as eForms, SDK 1.7: Doffin has converted its history, so there is one format family.

## What the notices look like (22 downloaded, 22 valid eForms)

| Aspect | Observed |
|---|---|
| Document roots | `ContractNotice` 17, `ContractAwardNotice` 4, `PriorInformationNotice` 1. The parser needs all three. |
| Flavour | `eu` 21, `national-e` 1. **`national` was not seen**, and recent competition notices were all above threshold, so this sample under-represents below-threshold notices. |
| SDK version | 1.14 (16), 1.13 (5), 1.7 (1) |
| Subtype | 16 competition (14), 29 contract award (2), 12 PIN used as a call for competition (1), 15 qualification system (1), 25 voluntary ex-ante transparency (1), 39 contract modification (1), 4 PIN only (1), **E3** below-threshold competition (1) |
| Versions | A contract modification (`can-modif`) is `VersionID` **02**, so versioning is real. |
| Procedure id | Every notice has `ContractFolderID` except the planning-only PIN, as DFØ rule R-15 says. |
| Document and submission links | Hosted on **Mercell** (12 of 22), **EU-Supply**, **Ivalua** (Sykehusinnkjøp), **Artifik** and **Achilles**, or on the buyer's own site. Tenders are submitted there, not in Doffin. |

Mapping from the API type to the document: `ANNOUNCEMENT_OF_COMPETITION` is a `ContractNotice`, `ANNOUNCEMENT_OF_INTENT` is a `ContractAwardNotice` (subtype `veat`), `ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT` and `CANCELLED_OR_MISSING_CONCLUSION_OF_CONTRACT` are `ContractAwardNotice` (`can-standard`), `CHANGE_OF_CONCLUSION_OF_CONTRACT` is a `ContractAwardNotice` (`can-modif`), and `ADVISORY_NOTICE` is a `PriorInformationNotice`. A `PRE_ANNOUNCEMENT` was a `ContractNotice` (`pin-cfc-social`).

The XML holds what the API summary lacks: each lot's submission deadline, the **question deadline** (`AdditionalInformationRequestPeriod`), the tender-opening time, award criteria with weights, selection criteria, planned period and extension options, and the contracting parties with their contact details.

### Reconciling the API with the XML

Rules learned from comparing the two on the same notices; the validation stage should encode them.

- API `cpvCodes` is a **superset** of the XML's codes: the API adds ancestor codes (it listed `79000000` and `79410000`, which the XML does not). Compare as "XML subset of API".
- API `heading` equals the XML project name in Norwegian, verbatim, double spaces included.
- API `deadline` equals the XML submission deadline in UTC on competition notices. On result notices it is **not** a tender deadline (it held dates in 2028), so it must never be read without the notice type.
- XML dates are `xs:date` with an offset (`2026-10-07+00:00`) and deadlines are written in UTC; they must be shown in Europe/Oslo.
- `buyer.organizationId` needs normalising to nine digits. One notice carried a company **name** in an organisation-number field (`CompanyID`), so the validation gate has to catch non-numbers.
- Notices contain **personal data**: contact persons' names, e-mail addresses and telephone numbers in clear text.

## TED

- **Verified:** the Search API answers anonymously. TED's own documentation says every service that reads *published* notices is anonymous and that a key is needed only for endpoints that touch notices not yet published (publication, validation). **The TED key in `.env` is not needed by anything anbud-core does, and nothing sends it.**
- `POST https://api.ted.europa.eu/v3/notices/search` with `{query, fields, limit, page, scope}`. The response carries `notices[]` with `links` to the XML and to PDFs in every language.
- **1830** selectable `fields`, among them `publication-number`, `BT-701-notice` (notice identifier), `BT-757-notice` (version), `BT-04-notice` (procedure identifier), `notice-type`, `publication-date`, `deadline-receipt-tender-date-lot`, `document-url-lot`, `submission-url-lot`.
- A notice resolves by its UUID: `BT-701-notice=<uuid>` (also `notice-identifier=<uuid>`).
- **Cross-source identity verified.** Of the 22 Doffin notices, **19 were found in TED on the same `notice-identifier`, all 19 with the same version and 18 of 18 with the same `procedure-identifier`** (the planning notice has none). No query returned more than one notice. The three missing are explained: a below-threshold `national-e` notice, a converted 2017 notice, and a PIN-CFC not yet published. Doffin identifiers and TED publication numbers (`698343-2026`) are unrelated sequences, which is why neither may serve as identity.
- Pagination bounds (250 per page, 15 000 in page mode) come from the existing Quarry collector and were not re-measured.
- **Loaded since 2026-10-28** (`doffin-sync -source ted`). Observed that day: the expert query `publication-date=YYYYMMDD AND buyer-country IN (SWE DNK FIN ISL)` works, and one day held 4 105 notices EU-wide, 283 for the Nordic buyers with Norway and 231 without. A notice's XML is `https://ted.europa.eu/en/notice/<publication-number>/xml`. A burst of 20 downloads drew five `429 Too Many Requests` HTML pages; one download every three seconds drew none in 50, so the client paces both calls at 20 a minute.
- **Publication date (2026-10-28).** TED's search returns each notice's `publication-date`, which the sync stores with the search page (`internal/sources/ted`): the catalogue's "Publisert" column shows it for TED notices, keyed by the `TED-` id, as it shows Doffin's publication date from Doffin's own search pages. A notice whose search page was stored before this change has no date until a recheck of its day (`-recheck N`) stores a new page.
- **TED publishes plain EU eForms**, Norwegian notices included: `cbc:CustomizationID` is `eforms-sdk-1.13` or `-1.14` with no national suffix. On a sample of 50 notices (40 Swedish, Danish, Finnish and Icelandic, 10 Norwegian), the parser read every contract and award notice of SDK 1.13 and 1.14 once plain EU eForms was mapped to the existing extractors (41 of 50; the 9 left were prior information notices, read since the same day through the contract notice's paths: TestPriorInformationNoticesReadLikeContractNotices), and the winners it read for all 23 award notices were exactly the tenderers TED's own search metadata names (`organisation-name-tenderer`). A suffix this code does not know is still refused.
- **Every held shape reads (2026-10-28).** Of the 1 056 held notices, 78 were refused, all from Doffin: contract notices of SDK 1.13 in the national-e (34) and national (4) flavours, and award notices of 1.13 eu (27, VEATs among them), 1.13 and 1.14 national-e (8) and 1.13 national (5). Mapped to the extractor of their kind, they were checked against Doffin's own search metadata: title and buyer in 54 of 54, the tender deadline of 29 of 29 contract notices, the winners of 17 of 17 award notices (TestEveryHeldDoffinShapeReads). Six prior information notices of the national flavour (SDK 1.13, subtype N4), found on Doffin between February and September 2026, read with the title and buyer of Doffin's metadata. Nothing of 1.14 national has been seen, neither among the held notices nor in about 50 sampled from Doffin across April to October 2026 (1.14 came as eu or national-e; national came as 1.13). It is mapped by analogy to the extractors that read its neighbours, and TestSDK114NationalReadsLikeItsNeighbours pins the analogy; the first real one will check it.
- **The merge key holds on live data (2026-10-28).** Of the 20 newest Doffin notices in the local store, all 20 were on TED under the same eForms notice identifier, with the same version (for example Doffin 2026-116215 is TED 722502-2026). The readers merge on that identifier.

## BOAMP (France)

BOAMP is the French official bulletin of public tenders, published by DILA. Its notices are open data on an Opendatasoft portal, read anonymously by `doffin-sync -source boamp` (`internal/sources/boamp`). Facts verified 2026-10-29.

- **Licence and attribution.** Licence Ouverte 2.0 (Etalab), which requires the source to be named as BOAMP (DILA). The app names the source ("BOAMP") on each notice; the attribution line itself is a go-live item, with the other sources'.
- **Access.** No key. The records endpoint is `https://boamp-datadila.opendatasoft.com/api/explore/v2.1/catalog/datasets/boamp/records`. The portal's own quota is 2 000 000 requests a day (`X-RateLimit-limit`), far above a daily sync; the client paces itself at 20 calls a minute, as it does for TED.
- **Search.** A day is `where=dateparution = date'YYYY-MM-DD'` (the date literal: a text comparison is refused with `IncompatibleTypesInComparisonFilter`), `order_by=idweb`, `limit` up to 100, `offset`. The portal refuses `offset + limit` above 10 000 (`InvalidRESTParameterError`, 10 100 was refused), so the client refuses a page past that window rather than sending it. The search keeps only `idweb`, `dateparution` and `famille` as evidence; the notice is the download's.
- **Identifier.** `idweb` (`26-104530`) is the notice's identifier and the source's own id; the public id is `BOAMP-26-104530`. The portal's record id (`26_104530`) does not look a record up: `records/{id}` answered 404 for both spellings, so a download is a search for `where=idweb="26-104530"`.
- **Shape of the record.** `donnees` is the notice's eForms XML converted to JSON: the root is `EFORMS` over `ContractNotice`, `ContractAwardNotice` or `PriorInformationNotice`, element prefixes are kept (`cbc:`, `cac:`, `ext:`, `efac:`, `efbc:`, `efext:`) and attributes are `@name` members. `internal/sources/boamp/eforms.go` writes it back as XML with the namespaces those prefixes stand for, in document order, so the body and its hash are the same on every download.
- **Not every record is eForms.** Of the 300 records published on 2026-10-29, 277 were eForms, 21 French national forms (`FNSimple`) and 2 `MAPA` forms. The non-eForms ones are kept as their own JSON (`application/json`), which the eForms parser refuses, as it refuses any unsupported notice.
- **SDK 1.12 is not read yet.** Of the 277 eForms records, 18 are SDK 1.12 (`eforms-sdk-1.12`); the parser has no field map for 1.12 (`internal/eforms/parse.go`), so those notices are stored and shown as unsupported. The other 259 were read (SDK 1.13 and 1.14), and every one converted. `TestCorpusConvertsAndParses` runs the whole captured day when `BOAMP_CORPUS_DIR` names it, and fails on anything but the known 1.12 gap.
- **Overlap with TED: none under the current configuration.** The readers show one copy of a notice held by two sources: the higher version, and on a tie the source ranked higher (Doffin, then TED, then BOAMP; `internal/demo/twins.go`). The overlap needs a shared buyer. TED's sync reads Nordic buyers only (`NOR, SWE, DNK, FIN, ISL`, the default of `-countries`), and BOAMP's notices are French buyers. On 2026-10-28, 132 stored BOAMP notices and 8 257 stored TED notices shared no eForms notice identifier and no contract-folder id. If TED's buyer countries are widened to include `FRA`, check the overlap again on a day both sources hold.

## Find a Tender (UK)

Find a Tender is the UK government's public procurement service. Its notices are published as OCDS release packages (Open Contracting Data Standard, version 1.1 with the EU profile) and read by `doffin-sync -source fts` (`internal/sources/fts`). Facts verified 2026-10-29.

- **Licence and attribution.** Open Government Licence v3.0. The licence asks for the source to be acknowledged; the attribution line ("Contains public sector information licensed under the Open Government Licence v3.0") is a go-live item, with the other sources'.
- **Access.** No key. `https://www.find-tender.service.gov.uk/api/1.0/ocdsReleasePackages` answers anonymously. The listing takes `updatedFrom`, `updatedTo`, `limit` and `cursor` (the service refuses any other parameter and names the allowed ones, among them `stages`). The client asks for 100 a page and paces the listing at 20 calls a minute. **The downloads are paced at 6 a minute:** on 2026-10-29 a run of ten downloads in about thirty seconds drew `429 Too Many Requests` with `Retry-After: 120`, and the engine paused the day (exit 3). The service's own limit is not published, so 6 a minute is a choice below the one observed refusal, not a measured ceiling.
- **Listing and paging.** A listing states no total: each page carries `releases` and `links.next`, an absolute link on the same host. The client walks a day's pages once, keeps that day's listing, and answers the engine's numbered pages from it, so the total the engine sees is the same on every sweep of the day. A next link to another host is refused.
- **The day is the day a notice was last updated.** The window is `updatedFrom` to the next midnight, not the publication day. A notice updated later appears again on the day of the update, and the day is what the sync stores. This is what the service offers; it is not a publication day in the TED or BOAMP sense.
- **Identifier.** A release's `id` is the notice number (`101800-2026`), the source's own id, with the public id `FTS-101800-2026`. The contract's `ocid` (`ocds-h6vhtk-0791ce`) is kept as the procedure identifier. The notice number has TED's shape, which is why the public id carries the prefix.
- **Download.** `ocdsReleasePackages/{noticeNumber}` returns the release package of that notice (one release, 200 with JSON). The client checks that the package holds the notice asked for, and keeps the JSON as the body (`application/json`). The per-notice lookup by OCID does not exist (the service answers 404 and 400).
- **Reading.** `internal/sources/fts/parse.go` maps the release to the eForms model the readers share: `tender` to a contract notice, `award` to a contract award notice, `planning` to a prior information notice, with the eForms notice-type codes `cn-standard`, `can-standard` and `pin-only`. It reads the title, description, buyer, value (in minor units; the net amount, or the gross where the net is missing), the tender period's end as the deadline, the main procurement category, the CPV codes of the classification and the items (eight digits, as eForms writes them), the lots and the document links. A tag the reader does not know is refused.
- **Not read for a UK notice.** The NUTS regions, the procedure type and the contracting authority's company number are not in the release the reader maps, so they stay empty and the readers show them as missing.
- **Quota (measured 2026-10-29, not yet explained).** A day holds 490 releases (2026-10-28). Downloads at 6 a minute, one every ten seconds, were refused with `429`, `Retry-After: 120` after 10 downloads in the first run and after 3 in the second, roughly 20 downloads over about ten minutes in total. A single unauthenticated probe four minutes later was still refused with `Retry-After: 120`. So the service enforces a quota over a window, not only a per-minute pace, and the engine pauses the day (exit 3) and resumes on the next run. The sustainable daily volume is not known. Before a backfill, either measure it over several runs, or find the service's bulk release files, which this client does not use.
- **Scale.** The day's listing is held in memory for one day at a time; the engine's accessible cap (1 000 a day) applies as it does to every source. 15 notices of 2026-10-28 are stored so far.

## BZP (Poland)

BZP, the Biuletyn Zamówień Publicznych, is the Polish public procurement bulletin. Its board search answers anonymously, and `doffin-sync -source bzp` reads it (`internal/sources/bzp`). Facts verified 2026-10-29 from `https://ezamowienia.gov.pl/mo-board/api/v1/Board/Search`.

- **Licence and attribution: not verified.** I did not check the reuse terms of the board's data before building against it. Check them before go-live, as the other sources' attribution lines are checked.
- **Access.** No key. The client paces both the listing and the downloads at 20 calls a minute. The board's own limit is not measured yet.
- **Listing by day.** `publicationDateFrom` and `publicationDateTo` take the same day (inclusive), and `PageSize` and `PageNumber` page the answer. The board answers **ten records a page whatever `PageSize` asks**, and it states no total. Deep pages fail: the search sits on Elasticsearch, which refuses a window past 10 000 records (a `PageNumber` of 99 999 returned a 500 with "Result window is too large").
- **The order of a day's pages is not stable.** Across 40 pages of 2026-10-28 there were 53 repeats of a record already seen, so paging once can miss notices. The client reads the day again while a pass finds a notice the last one did not (up to three passes), and keeps each notice once. No sort parameter changed the order (`sortBy`, `orderBy` and `sort` were tried and ignored).
- **Download by number.** The board's `noticeNumber` filter returns exactly that notice. The filters `moIdentifier`, `objectId`, `bzpNumber` and `tenderId` are ignored and return the default page, so they cannot look a notice up.
- **Identifier.** The notice number is `2026/BZP 00505976/01` (year, number, version). The source's own id is `2026-00505976-01`, and the public id `BZP-2026-00505976-01`. A planning notice's number ends in `/P`, which the id carries as `-P` (`2025-00610043-06-P`).
- **Which notices the sync reads.** Notices whose `bzpNumber` carries `/BZP ` and whose `noticeType` is one of three: `ContractNotice` (an opportunity), `TenderResultNotice` (an award, the board's name for a result) and `TenderPlanNotice` (a planning notice). `NoticeUpdateNotice` and `ContractPerformingNotice` are left out.
- **What the board lists that is not BZP's.** Most of a day's records on 2026-10-28 are of a different kind: a `null` notice type, a `bzpNumber` in TED's form (`2026/S 209-745248`), and a `pdfUrl` on `ted.europa.eu`. These are notices a Polish buyer published on TED. The sync leaves them out. TED's own sync reads no Polish buyer (`-countries` defaults to `NOR,SWE,DNK,FIN,ISL`); adding `POL` there would bring them in as eForms XML, which is the better way to read them.
- **Reading.** `internal/sources/bzp/parse.go` maps the record to the eForms model: the title from `orderObject`, the buyer from `organizationName`, the deadline from `submittingOffersDate`, the procedure from `tenderId` (the OCDS identifier), the procurement type from `orderType`, and the CPV codes from `cpvCode` (eight digits, once each, without the labels). The record carries no contract value, so the value and the descriptions stay empty. The buyer's Polish tax number (`organizationNationalId`) is not used: the readers take Norwegian company numbers for their lookups.
- **TLS.** `curl` accepts the board's certificate chain on this machine, and Python's `urllib` refuses it ("self-signed certificate in certificate chain"). The chain is trusted by the macOS keychain and not by Python's bundle. The client does not disable verification; Go's client uses the system roots on macOS, the same as `curl`. Check the first live run.

## Volumes and budget

- 159 833 notices have been reachable since **2017-01-02**, 3 567 days, about **45 per calendar day** on average and about 100 on a busy recent day.
- A one-day window (`issueDateFrom`=`issueDateTo`) always fits under the 1000-hit cap, so a full backfill is one date window per day: roughly 3 600 days times one to ten pages, 2 to 7 hours of search at 24 per minute.
- Downloading every notice at 8 per minute would take about two weeks, so history comes from metadata and the year CSV, and XML is fetched for recent, active or matched notices only. A normal day (about 100 notices) costs about 13 minutes of the download budget. The day-window sync's arithmetic, including its two-pass search rule, is in [ARCHITECTURE.md](ARCHITECTURE.md#budget-arithmetic).

## Doffin Notices API (not used)

`eform-api` on the same portal. It **publishes** notices to TED and Doffin; submissions are legally binding and cannot be deleted, and the subscription must be activated manually by DFØ. A reader never needs it, and this service must not hold its key.

## Doffin year CSV (backfill)

The existing source-only collector in `Quarry-v2/services/quarry-control/internal/procurement` reads `https://adaapnedataprodst.blob.core.windows.net/kunngjoringer/{year}/Kunngjoringer_{year}.csv`. **Unverified:** availability and cadence per year, and the CSV's terms. Useful for bulk backfill and reconciliation. That package's comment, "the catalog publishes no read API", is outdated.

## Scraper (Quarry-v2)

`doffin.no` pages are a single-page app: a plain fetch returns an empty shell. `robots.txt` (checked 2026-10-09) allows everything (`Disallow:` empty). The site's terms of use have **not** been checked. Tender documents live on the platforms listed above (Mercell first), so the scraper's real job is reaching those, not Doffin. Whether Mercell's `permalink` pages expose the documents without a login is **unchecked**.

## Identifier model (DFØ, "Notice identifiers" v0.1, 2026-02-17)

Source: `anskaffelser/eforms-sdk-nor`, `docs/notice-identifiers.md`. Normative rules this service follows:

| Identifier | Level | Stability | Use |
|---|---|---|---|
| `doffin-identifier` | publication reference | per publication | Doffin lookup only. Not part of the eForms payload. |
| `publication-number` | publication reference | per publication | TED lookup only. |
| `notice-identifier` | eForms semantic | stable across versions | Notice identity. |
| `notice-version` | eForms semantic | changes on republication, including corrigenda | Version within a notice identity. |
| `procedure-identifier` | eForms semantic | stable across notices, phases and corrigenda | **The grouping key for a procedure.** |

- Never use a notice-level identifier as a procedure key (R-2, R-4, R-14), and never treat `doffin-identifier` or `publication-number` as a substitute for (`notice-identifier`, `notice-version`) (R-6, R-21).
- Reference a specific notice version with **both** `notice-identifier` and `notice-version` (R-7).
- A procedure exists once a competition notice is published (R-11). A planning notice (`PriorInformationNotice`) carries **no** `procedure-identifier` and is linked to later notices by referencing the earlier notice's (`notice-identifier`, `notice-version`) (R-15 to R-19).

## eForms flavours

Told apart by the suffix on `cbc:CustomizationID`; `internal/eforms.Sniff` reads it.

| Suffix | Meaning | Seen |
|---|---|---|
| `#extended#urn:fdc:anskaffelser.no:2023:eforms:eu` | Submitted to TED (above threshold or voluntary) | 21 of 22 |
| `#extended#urn:fdc:anskaffelser.no:2023:eforms:national` | Norwegian national notice (below threshold) | none yet |
| `#extended#urn:fdc:anskaffelser.no:2026:eforms:national-e` | New below-threshold notices | 1 of 22 (subtype E3, SDK 1.14), already in production |

The parser must select field maps and code lists from the eForms SDK version and flavour found there, and fail closed on an unknown one.

## Live smoke test (2026-10-10)

V1 to V4 ran on 2026-10-10 against the live API with the project's key. They used the built `doffin-sync` and a scratch PostgreSQL 16.14 database, `anbud_smoke`, in a throwaway container that was removed afterwards. Each live call had the owner's go-ahead, and the key was loaded from the environment and never printed. The results below are observed. V5, the census of the full history, has not run and needs separate approval.

| Test | Settles | Calls | Status |
|---|---|---|---|
| **V1** Inclusive bounds | Whether `issueDateFrom` and `issueDateTo` both include their day. The closed-day rule and the one-day window depend on it. | 3 searches with `numHitsPerPage = 1`. Query A is [D, D], B is [D+1, D+1], and C is [D, D+1]. Pass if `C.total = A.total + B.total`, and A and B are each at least 1. | **Passed**, on four day pairs: 92 + 94 = 186 (Wed 7 and Thu 8 October), 62 + 75 = 137 (Mon 5 and Tue 6), 61 + 31 = 92 (Tue 29 and Wed 30 September) and 19 + 2 = 21 (Sat 3 and Sun 4 October). Both bounds include their day. |
| **V2** Paging with ties | Whether pages are stable under tied `PUBLICATION_DATE_ASC` keys. If a busy day's pages are not stable, it stays partial on every run and needs another sort order. | 2 to 3 searches, zero downloads. Pick a day with 11 to 30 hits and use `numHitsPerPage = 10`. Check that `T` and `A` match on every page, and that the distinct ids equal `T`. | **Failed on a multi-page day.** 19 hits at 10 per page passed: 2 pages, 19 distinct ids. 94 hits at 25 per page did not: 4 pages gave 65 distinct ids, 29 of them repeated. A second try gave one perfect sweep and one whose pages 2 and 3 shared no id with the first sweep's. At 50 per page, four sweeps (two sorted by `PUBLICATION_DATE_ASC`, two by `DEADLINE`) were identical. The union of two sweeps held all 94. See [Tie order](#what-the-smoke-test-found). |
| **V3** Smoke sync | The whole path on real bytes: the stored body's hash, the rows written, and the day and run states. | 1 search and 1 download: `-from D -to D -max-downloads 1` against a scratch database. Expect exit 2, the day `partial` with `download_limit`, one stored row, the hash check `t`, one search-page row, one `stored` attempt, the run `partial`, and `attempts = 1`. The day needs at least two notices. With one, the day completes and the exit is 0. | **Passed.** Exit 2. The day was `partial` with `download_limit`, 19 of 19 counted, 1 stored. The run was `partial` with 2 searches and 1 download. One body of 25,084 bytes whose hash matches, one search-page row, one `stored` attempt, `attempts = 1`. The two searches are the two sweeps; they returned the same bytes, so one row. |
| **V4** Resume (optional) | That a rerun skips the notice already held and downloads only the rest, with no duplicate rows. | 1 more search and 1 more download, by running V3 again. Expect `held = 1` and two raw rows with distinct ids. | **Passed.** Exit 2. `held = 1` and `held_skipped = 1`, one new body of 33,465 bytes, two raw rows with distinct ids and distinct bodies, `attempts = 2`, two `stored` attempts. The second run's search page differed in bytes from the first's (see `allTypes` below), so the page table holds two rows. |
| **V5** Census of the full history (optional) | The real download backlog and the number of capped days, before any budget is committed. It decides the backfill window. | Every closed day since 2017-01-02 is searched, with no downloads. The plan's one-pass estimate is about 3,567 to 5,165 searches (2.5 to 3.6 hours). Because every day is swept at least twice, expect roughly double: about 5 to 7 hours, plus the extra sweeps of any multi-page day (D-B3). | Not yet run. Needs separate approval. |

Not exercised live: 404 responses, 429 responses, the saturated path, refused bodies (empty, or over the size cap), and any multi-page day at the production page size of 100 (no day sampled held more than 94 notices). Only the loopback tests cover them.

The smoke test made 50 searches and 2 downloads in about an hour, at the default pacing of 24 searches and 8 downloads per minute, and drew no 429. That is twice the 25 searches planned. The extra searches went on finding, reproducing and then proving the fix for the V2 failure.

### What the smoke test found

- **Tie order is not stable between requests (V2).** Every hit of a one-day window has the same publication date, so `PUBLICATION_DATE_ASC` leaves them tied, and the server does not keep tied hits in one order between requests. On the day of 94 hits at 25 per page, one sweep returned every id and the next returned 65 distinct ids with 29 repeats. Pages 1 and 4 were identical between the two sweeps and pages 2 and 3 shared no id, which fits a large block of tied notices published together in the middle of the order. A census of the fixed engine at 25 per page needed three sweeps (12 searches), and each page number came back in exactly two variants of different sizes. The order was stable on the 19-hit Saturday and for four sweeps at 50 per page, so the instability is intermittent. A sweep over a day that fits on one page of 100 is not affected, and every day sampled did. The engine's answer is the union of sweeps (D-B3 in [ARCHITECTURE.md](ARCHITECTURE.md)). Sorting by `DEADLINE` was no more stable in principle: notices without a deadline tie there too.
- **The date filter selects by `publicationDate`.** All 19 hits for 2026-10-03 carry `publicationDate` 2026-10-03 and an `issueDate` timestamp on 2026-10-02 (between 04:50 and 18:28 UTC). The parameters are named `issueDateFrom` and `issueDateTo`, but they filter on the publication date, a plain date with no time zone in it. In this sample every notice published on Saturday 3 October was issued on Friday 2 October, which fits the thin weekend (19 hits on Saturday, 2 on Sunday). Which zone assigns `publicationDate`, and when in the day, is not known. The closed-day margin is unchanged.
- **`allTypes` changes order between fetches.** Between two fetches of the same page, 16 of 19 hits, the ones with more than one type, listed `allTypes` in a different order, and nothing else differed. The bytes differ, so the raw store keeps both copies, because its identity includes the body hash. A search page of unchanged content can be stored several times. A parser must treat `allTypes` as a set.
- **Volumes.** Hits per day on the days sampled: 19 (Sat), 2 (Sun), 62 (Mon), 75 (Tue), 92 (Wed), 94 (Thu), and 61 and 31 on Tue 29 and Wed 30 September. The busiest, 94, fits on one page.
- **Sizes.** A search page of 19 hits was 54,881 bytes, about 2.8 KiB per hit. Pages of 25 hits on the 94-hit day were 45 to 85 KB. The two downloaded notices were 25,084 and 33,465 bytes.
- **The first two notices the sync stored** are `ContractNotice`, notice type `cn-standard`, subtype `E3`, flavour `national-e` (the 2026 below-threshold flavour), eForms SDK 1.14 and 1.13. Both pass `eforms.Sniff`. The first corpus sample held one `national-e` notice in 22; the first two downloads of a Saturday were both `national-e`. The parser's field maps must cover it from the start.
- **A `stored` attempt carries no HTTP status.** The status column is filled for failures only.

### Other open questions the tests above do not settle

- Which zone assigns `publicationDate`, and at what hour does a notice get the next day's date? The filter selects by that plain date (V1, and the smoke test above), so there is no zone inside the query. The closed-day margin assumes a zone at or ahead of UTC. Not tested.
- Do notices appear on a closed day after that day? The planned daily recheck covers four days, and anything later is missed. Not tested.
- Do withdrawn notices return 404 on download, and do they come back later? The seven-day grace period depends on the answer. Not tested.
- Do bodies change under a known id? That is undetectable without a download, and the design accepts the loss. Not tested.
- Do 404 or 429 responses count against the budget? Unknown. Not tested.
- Why would a download come back empty or over the size cap, and does the same notice recover later? The bounded retry (three refusals at least 24 hours apart, then terminal) assumes a refusal can be transient. Not tested.
- Are there quotas beyond the per-minute limits? The backfill assumes none. Not tested.
- How are body sizes distributed? The sample holds 8 KB to 540 KB. The two bodies the sync stored were 25,084 and 33,465 bytes.
- How many days hold more than 100 notices, and how often is their tie order torn? None of the 8 days sampled passed 94. The census (V5) would count them, and each day line's `searches` would show how many sweeps a busy day needs.

## Parser and document links (2026-10-10)

**Parser, stage 2 (`internal/eforms`, `Parse`).** Reads a notice into a `Notice`: title, value, CPV codes, NUTS places, submission deadline, buyer name and organisation number, document links, qualification requirements and lots. Only the shapes in its field maps are read: ContractNotice, SDK 1.14, flavours `eu` and `national-e`, and (since the second pass below) ContractAwardNotice, SDK 1.14, flavour `eu`. Any other shape is refused with `ErrNoFieldMap`, not guessed at.

Corpus, 22 captured notices, counts only: 11 parsed (all 10 `eu` and the one `national-e` of SDK 1.14), 11 refused for having no field map (5 ContractNotice SDK 1.13, 1 SDK 1.7, 4 ContractAwardNotice, 1 PriorInformationNotice). Of the 11 parsed: CPV, NUTS, title, buyer name and organisation number, contact count, document link, requirement, lot and lot deadline are present in all 11. One buyer organisation number is not digits and is reported as an issue. Notice-level deadline and notice-level value are absent in these notices; the lot deadline and the lot-level structure carry them. Lot values are absent in all 11.

Where the data sits, from element names and value shapes only (no values were printed):

- The buyer is a party that refers, by an identifier of the shape `ORG-nnnn`, to an organisation in the notice's extension block (`efac:Organizations`). Its organisation number, name and contacts are there, not in the party.
- Contact persons (name, e-mail, telephone) are in that organisation list. The parser counts them and never copies them.
- Document links are per lot: `cac:TenderingTerms/cac:CallForTendersDocumentReference/cac:Attachment/cac:ExternalReference/cbc:URI`.
- Qualification requirements are per lot and at notice level: `cac:TenderingTerms/cac:TendererQualificationRequest/cac:SpecificTendererRequirement`, with a type code and a description. Whether a requirement is mandatory is not stated by any field this parser reads; the eForms path for it is not yet found, so the gate cannot yet tell mandatory from optional.
- Deadline dates carry their zone (`2026-11-15+01:00`), and times may have fractional seconds.
- Award criteria and their weights are per lot: `cac:TenderingTerms/cac:AwardingTerms/cac:AwardingCriterion/cac:SubordinateAwardingCriterion`, the weight in its own extension block (`efac:AwardCriterionParameter`). Read since the second pass below.

Not read yet, and reported in `Unmapped` per notice: issue and publication dates, submission method, question deadline, contracting activity, lot-level places, selection criteria, post-award process.

**Second pass: award notices, award criteria, procedure type (2026-10-10).** Mapped from element names, value shapes and code-list codes of the 4 captured award notices, no values printed.

- **Result.** In the extension block: `efac:NoticeResult`. A lot result (`efac:LotResult`) names its lot (`efac:TenderLot`), its winner-selection status (`cbc:TenderResultCode`, list `winner-selection-status`: `selec-w` winner chosen, `clos-nw` closed without a winner) and the winning tenders by reference (`efac:LotTender/cbc:ID`). A winner is reached by references only: lot result → `efac:LotTender` → `efac:TenderingParty` → `efac:Tenderer` → the organisation list, where its name and organisation number are. A reference that leads nowhere is an issue and gives no winner. Subcontractors (`efac:SubContractor`) are not winners and are not read.
- **Tender count.** `efac:ReceivedSubmissionsStatistics`, list `received-submission-type`. Only the code `tenders` is the count of tenders; the others (`t-sme`, `t-esubm`, `t-oth-eea`, `t-no-eea`, …) are subsets of it and stay unread.
- **Values.** `cbc:TotalAmount` of the result, and each tender's `cac:LegalMonetaryTotal/cbc:PayableAmount`. A lot's value is the winning tender's only when the lot has one winning tender.
- **Award date.** `cac:TenderResult/cbc:AwardDate`, read as its calendar day.
- **Weights.** Read only when the parameter is coded `per-exa` (an exact percentage, list `number-weight`) and is a whole number from 0 to 100. Any other parameter (an order of importance, points) gives no weight, so a rank is never shown as a share.
- **Procedure type.** `cac:TenderingProcess/cbc:ProcedureCode` (list `procurement-procedure-type`), for both shapes.

Corpus after the second pass, counts only: 15 of 22 parse (the 11 contract notices and the 4 award notices); 7 refused (5 ContractNotice SDK 1.13, 1 SDK 1.7, 1 PriorInformationNotice). Of the 15: procedure type 14; award criteria in 3 notices, a percentage weight in 1. Of the 4 award notices: result 3, award date 4, tender count 2, winner 2 (both with an organisation number), awarded total 1. Award criteria are rare in the contract notices of this sample: the weights a bid manager needs are mostly in the tender documents, not the notice.

**Selection criteria (2026-10-10).** What `Notice.Requirements` holds (`cac:TendererQualificationRequest/cac:SpecificTendererRequirement`) is not the qualification criteria: across the 15 parsed notices the 235 entries are ESPD exclusion grounds (`exg-*`, 27 codes, 7 each), pointers (`epo-procurement-document` 31, `epo-notice` 7, `epo-acc-espd-request` 1) and `none` 28. The criteria themselves are `efac:SelectionCriteria` in the extension block of a lot's `cac:TenderingTerms`, with `cbc:TendererRequirementTypeCode` (list `selection-criterion`, such as `slc-abil-ref-services`, `slc-suit-reg-trade`) or `cbc:CriterionTypeCode` `other`, a usage code (`used`/`not-used`), and `cbc:Name` and `cbc:Description` per language (`NOR`, `ENG`). The parser reads them since this pass (`Notice.SelectionCriteria`, Norwegian text preferred). In the corpus they occur only in SDK 1.13 notices (4 of 5, 9 criteria), which had no field map at that point (they do since the third pass below); no SDK 1.14 notice states one, and all 11 parsed 1.14 tender notices point to the tender documents.

**Normalisation (`internal/normalize`, 2026-10-10).** Over the 15 parsed notices, counts only: no CPV, NUTS or organisation-number correction was needed and none failed (every organisation number the parser kept has a valid modulus-11 check digit); 11 have a next deadline (the 4 award notices have none, as expected). (The one organisation number the parser then dropped as "not digits" is explained in the third pass below: it is not a VAT spelling, it is junk.)

**Document links, measured (`cmd/link-probe`, 2026-10-10).** The probe reads the 22 captured notices, takes each distinct external-reference link once, skips the known login platforms without requesting them, and asks the rest once with a range request, no cookies, no login and at most three redirects. It prints counts only.

- 22 notices read, 0 refused; 18 hold external links, 18 distinct links.
- By host family: login platform 14 (not requested), Norwegian host other than Doffin 3, other 1, Doffin 0.
- Requested 4 (the three Norwegian hosts and one other): all 4 returned an HTML page, none a document, none a login form on the first 2 KB.

What this says: most document links in this corpus go to login platforms, where the user must sign in. The four links that could be asked returned pages, not documents; whether those pages link to a document needs one more hop, which is not done. The probe does not show whether login-platform links are open, because they were not asked, by design.

**Second hop (`cmd/link-probe -hop-max 5`, 2026-10-10).** The four pages that answered with an HTML page were read once each (512 KB at most), and their document links on their own host were asked once each; no link on another host was followed, and no further hop was made. Counts only:

- 4 pages read, 0 unreadable.
- 1 document link on the same host across the four pages; 0 on other hosts.
- That link answered with a document.

So one document of the 18 distinct links is one click away, from one of the four buyer pages. The open-web route yields a document for a small minority of links on this sample.

**Third pass: SDK 1.13, deadlines for requests to participate, registry identifiers (2026-10-10).**

- **SDK 1.13.** The five captured ContractNotice SDK 1.13 notices (flavour `eu`; notice types `cn-standard` three, `pin-cfc-social` one, `qu-sy` one) were first read with the 1.14 field map without registering it, counts only, and compared with a separate structural survey of the same files, written independently of the parser (element names and value shapes only). Every count that can be compared agreed: a NUTS code in 3 of 5, a procedure code in 3, a tender deadline in 2, a deadline for requests to participate in 1, selection criteria in 4 (9 criteria), award criteria in 2. The two notices with no procedure code and no deadline are the `pin-cfc-social` and `qu-sy` types, which state none. The 1.14 extractor is therefore registered for ContractNotice, SDK 1.13, `eu`. The `national-e` flavour of 1.13 was not seen and is refused. The one SDK 1.7 contract notice is not registered: one sample cannot check a field map.
- **Deadline for requests to participate (BT-1311).** `cac:TenderingProcess/cac:ParticipationRequestReceptionPeriod` (EndDate, EndTime), at notice and lot level, is the deadline a restricted procedure states instead of one for tenders: tenders are invited later, and only from those who asked in time. One captured notice (SDK 1.13) states only this one. The parser reads it (`ParticipationDeadline`), the normaliser's `Deadlines` takes it as the binding deadline of its scope when stated (a company that has not asked cannot tender), and the gate names it: the check cites `participation deadline`, and its detail says "to request to participate". Without this, that notice would have been judged "no deadline".
- **Registry identifiers.** The 1.13 notices list many organisations whose identifiers are not nine digits: shaped like Swedish (`nnnnnn-nnnn`) and Finnish (`nnnnnnn-n`) registry numbers, and letter-and-digit codes. That was 31 parse issues under the old digits-only rule, mostly from parties other than the buyer: across the whole corpus only one buyer identifier is not a valid Norwegian organisation number (the country is inferred from the shape, because the organisation's country element is not read). The parser no longer judges the number. It keeps what is shaped like an identifier (letters, digits and `.` `/` `-`, at least one digit, at most 64 characters) and `normalize.OrgNumber` says whether it is a Norwegian organisation number, with a recorded problem when it is not. What the parser still refuses is what is not an identifier: in the corpus two values (a word of 14 letters and a single letter), reported as an issue and dropped.
- **Corpus after the third pass, counts only.** 20 of 22 captured notices parse (16 contract notices: 10 + 1 of SDK 1.14 and 5 of 1.13; 4 award notices); 2 are refused (the SDK 1.7 contract notice, the PriorInformationNotice). Normalised: 14 of the 20 have a deadline (the 4 award notices and the `pin-cfc-social` and `qu-sy` notices have none); problems found: one buyer identifier that is not a Norwegian organisation number, one extra-regio NUTS code. Selection criteria: 9 in 4 notices (codes `slc-abil-ref-services`, `slc-abil-staff-yrly-avg-mp`, `slc-abil-mgmt-qual`, `slc-suit-reg-trade`, `slc-stand-other` twice, and `other` three times). **14 of the 20 notices say their criteria are in the tender documents.** The placeholder requirement rules recognise 2 of the 9 criteria (Achilles, ISO 9001); the other 7 are "unknown" and the gate says so. The documents, not the notices, hold what a bid manager needs.

**Contract periods (2026-10-12).** The parser reads each lot's contract period (`cac:PlannedPeriod`: start and end dates, or `cbc:DurationMeasure` with a `unitCode` from the duration-unit list, or the timeperiod code `UNLIMITED`), the maximum renewals (`cac:ContractExtension/cbc:MaximumNumberNumeric`) and, in award notices, the day each contract was concluded (`efac:SettledContract/cbc:IssueDate`, reached from the lot result by the contract's id). In the captured corpus, counts only: of the 20 notices parsed, 18 state a lot period, 5 state renewals, and 2 of the 4 award notices state a signing date. Units seen: MONTH only. Dates come with a zone (`Z` or `+02:00`), read as the calendar day written.

**Change notices (2026-10-13).** Of the 22 captured notices, 3 carry a change section: a contract modification (`can-modif`, VersionID 02, an award notice), a prior-information notice change, and a qualification-system change. Each names the notice it changes by identifier and version (`efbc:ChangedNoticeIdentifier`), with a reason code (`change-corrig-justification`, values seen: `update-add`, `mod-cir`) and a reason in Norwegian and English. None of the three references points at another captured notice, so the rule that confirms a change against the notice it replaces has been tested with fixtures only. Every one of the 20 parsed notices states an issue date and time (`cbc:IssueDate`, `cbc:IssueTime`), in forms with a zone (`+02:00`, `Z`) or with fractional seconds.
