# Five-Window Replay Evidence

- Executed at (UTC): 2026-05-03T13:00:19Z
- Tenant: 33f319b9-4d57-11ee-960c-025417856183
- Endpoint: organizations

## Window Runs

| Window (UTC) | Status | Records In | Records Written | Started At | Finished At |
|---|---:|---:|---:|---|---|
| 2026-04-27 | succeeded | 1 | 0 | 2026-05-03 13:00:25.501269+00 | 2026-05-03 13:00:26.937179+00 |
| 2026-04-28 | succeeded | 1 | 0 | 2026-05-03 13:00:40.944686+00 | 2026-05-03 13:00:42.893796+00 |
| 2026-04-29 | succeeded | 1 | 0 | 2026-05-03 13:01:00.420888+00 | 2026-05-03 13:01:01.790489+00 |
| 2026-04-30 | succeeded | 1 | 0 | 2026-05-03 13:01:20.413964+00 | 2026-05-03 13:01:21.294691+00 |
| 2026-05-01 | succeeded | 0 | 0 | 2026-05-03 13:01:25.819579+00 | 2026-05-03 13:01:26.621584+00 |

## Post-Run Checkpoint

| Tenant | Endpoint | Source | Last Modified Watermark | Records Seen | Updated At |
|---|---|---|---|---:|---|
| 33f319b9-4d57-11ee-960c-025417856183 | organizations | erp_rest | 2026-04-27 23:59:59+00 | 0 | 2026-05-03 13:01:26.615227+00 |

## Out-of-Window Validation

| Check | Value |
|---|---:|
| Rows with lastModifiedDateTime > 2026-05-01T23:59:59Z | 0 |

## Script-Scoped Run History

| Started At | Status | Records In | Records Written | Finished At |
|---|---:|---:|---:|---|
| 2026-05-03 13:00:25.501269+00 | succeeded | 1 | 0 | 2026-05-03 13:00:26.937179+00 |
| 2026-05-03 13:00:40.944686+00 | succeeded | 1 | 0 | 2026-05-03 13:00:42.893796+00 |
| 2026-05-03 13:01:00.420888+00 | succeeded | 1 | 0 | 2026-05-03 13:01:01.790489+00 |
| 2026-05-03 13:01:20.413964+00 | succeeded | 1 | 0 | 2026-05-03 13:01:21.294691+00 |
| 2026-05-03 13:01:25.819579+00 | succeeded | 0 | 0 | 2026-05-03 13:01:26.621584+00 |
