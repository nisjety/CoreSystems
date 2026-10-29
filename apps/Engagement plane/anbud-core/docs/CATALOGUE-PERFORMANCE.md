# Prepared catalogue

The catalogue no longer downloads held XML from PostgreSQL and reparses every body
for each search. `catalogue_projection` is a rebuildable, versioned public list
projection keyed to the source row. The API backfills pending rows before serving
searches, warms a single immutable snapshot, and checks for new/changed source
records every five seconds. Source evidence remains append-only.

Search, exact global sorting, deduplication and self-excluding facet counts still
run over the complete prepared metadata in process. They are not SQL-paginated;
SQL-native filtering is a next step if corpus growth makes this CPU work material.
There is one bounded corpus snapshot per API process, not an unbounded query cache.
Each request checks the database generation. Imports, projection updates, publication
evidence changes and redactions invalidate it. Requests recalculate deadline status
against the current clock rather than retaining the day a row was prepared.

The preparer persists only public list fields and normalized deadline candidates.
It does not persist profiles, document contents or unverified winner names.
Projection writes lock the live source through an owner-controlled trigger; erasure
deletes the projection in the erasure transaction. Runtime code receives no UPDATE
privilege on source evidence. Prepared reads also join live, latest source rows.
Increment `catalogueProjectionVersion` when parser or projected fields change.

Publication dates are copied from held search evidence by the worker. A new import
or source date can take one polling interval plus preparation time to appear.
Winner verification for a newly prepared notice happens off the interactive path;
opening a detail still reads the source and performs the existing verification.

The frontend keeps eight recent pages in component-local memory, scoped to the
active organisation, for up to sixty seconds. Restored pages always revalidate;
no content goes into localStorage/sessionStorage. Existing rows remain visible
with an updating indicator, and next-page prefetches are cancellable.

Measured locally on 2026-10-29: 10,844 source records / 9,296 deduplicated entries.
Before: 11,606–12,500 ms per search. After: first snapshot load 359 ms; repeat
61–102 ms (direct API, full response read). Browser checks with real catalogue
responses measured 83–177 ms server fetches. These are local samples, not a
production percentile guarantee. Browser auth/workspace fixtures were mocked;
real-user login/gateway timing is outside those browser measurements.

Matching now has its own prepared facts alongside the list projection; see
[MATCHING.md](MATCHING.md). List requests do not load those matching payloads.
