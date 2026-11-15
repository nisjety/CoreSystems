# Audit evidence pack

Everything behind the three audit documents, so each claim can be traced. Nothing here was used to change the product or the reference apps.

Per audit folder (t1 = Space, t2 = Quarry and browser, t3 = Model Plane, chat and agents):

- `<source>.inventory.md`: first-pass inventory of each reference source.
- `<source>.<facet>.features.json` and `.deep.md`: structured feature records (with evidence and provenance) and the narrative deep dive per source and facet.
- `v-*.baseline.md` and `v-*.baseline.capabilities.json`: the code-verified baseline of what Verevon has today.
- `cmp/`: per-category cross-source comparison (clusters, best in class, Verevon mapping).
- `cons/` and `final/`: consolidated recommendations, tier calibration, waves, `items.verified.json` (every recommendation with its verification).
- `verify/`: raw outputs of the two verification passes (Verevon-side code reading, reference-side refutation).
- `critique/`: the two independent critiques of each document.
- `doc/`: the section files the documents were assembled from.

Top level: `verification-summary.json`, `critique-apply-report.json`, `assemble-report.json`.
Record ids such as `claude-product-ux-112` are `<source>-<facet>-<number>` and can be looked up in the matching `.features.json`.
