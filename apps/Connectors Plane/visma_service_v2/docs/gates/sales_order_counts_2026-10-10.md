# Sales orders per company: Visma against silver (2026-10-10)

Rule: FAIL when more orders are missing from silver than Visma reports as created or
changed since the last sync. See scripts/verify_sales_order_counts.py.

| Tenant | Visma | Silver | Missing | Changed since sync | Last sync (source) | Verdict | Why |
|---|---:|---:|---:|---:|---|---|---|
| Aquatiq AS (pilot) | 22,314 | 22,105 | 209 | 336 | 2026-10-01T13:50:35.570527+00:00 (last staging write (no heartbeat yet)) | PASS | 209 missing, within the 336 changed since the last sync |
| Aqua Pharma AS | 49 | 49 | 0 | 0 | 2026-10-01T14:10:40.679640+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aqua Pharma Group AS | 0 | 0 | 0 | 0 | none (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq AB | 69 | 69 | 0 | 0 | 2026-10-01T14:00:55.545070+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Aps | 13 | 13 | 0 | 0 | 2026-10-01T23:31:40.635391+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Food Automation ApS | 0 | 0 | 0 | 0 | none (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Group AS | 0 | 0 | 0 | 0 | none (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Sensilist AS | 193 | 190 | 3 | 4 | 2026-10-01T13:55:36.377930+00:00 (last staging write (no heartbeat yet)) | PASS | 3 missing, within the 4 changed since the last sync |
| Consult AS | 1,157 | 1,157 | 0 | 0 | 2026-10-01T11:42:31.034168+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Food Factory AS | 220 | 220 | 0 | 0 | 2026-10-01T12:51:33.790032+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Hygiene Systems AS | 3,497 | 3,497 | 0 | 0 | 2026-10-01T11:48:00.907636+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
| Aquatiq Sense AS | 77 | 77 | 0 | 0 | 2026-10-01T11:22:53.216519+00:00 (last staging write (no heartbeat yet)) | PASS | 0 missing, within the 0 changed since the last sync |
