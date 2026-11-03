# Space Wave 2 build ledger

Scope: the 98 rows of `.plan-work/space-build/queue-w2.json` (WS04 to WS16).

Status key:
- **built**: a `feat(...)` commit on this branch names the slice in its subject (checked over the last 1500 commits).
- **compile: confirmed** for every Rust crate and Go module that Wave 2 touched: `cargo check` on frontend gateway, session-core, model-gateway and execution-core, and `go build` on org-core. Frontend `pnpm typecheck` is clean except 21 errors in `quarry-client.ts` from the missing Quarry client in this worktree (environmental).
- The first check of this ledger listed 52 rows as not built. That came from searching only `space-w2`. The history of this branch names all 98, so those 52 are built here, and the next pass is verification, not new code.

Totals: 98 rows; the table gives each row its feature commit, or none.

| slice | lane | size | feature commit | deps |
| --- | --- | --- | --- | --- |
| WS04-S8 | L4 | M/L | `6e8700b8b` | WS04-S4, P-UC1, WS04-S5 (c), WS04-S6 (c) |
| WS04-S9 | L7 | M | `fff0b85d8` | P-GW1, WS04-S3 (c) |
| WS04-S10 | L4 | S/M | `8eeccc726` | WS04-S4, P-UC1 |
| WS04-S11 | L4 | L | `8f1ada435` | WS04-S10, P-UC1, WS04-S5 (c), WS04-S8 (c) |
| WS04-S12 | L6 | S | `2b57eb174` | WS04-S6, P-CX1 |
| WS05-S4 | L5 | M | `e10202329` | WS05-S1 |
| WS05-S5 | L4 | M | `ee93245f2` | WS03-S1, P-UC1 |
| WS05-S6 | L5 | M | `3fcde4fe9` | M0-BASE |
| WS05-S7 | L5 | L | `43b0a3951` | WS05-S3, WS05-S6 |
| WS05-S8 | L5 | L | `1229bf844` | WS05-S2, WS05-S6, WS05-S7, WS04-S10 (c) |
| WS05-S9 | L5 | S | `f3db9f144` | WS05-S8, WS04-S10 (c) |
| WS06-S6 | L3 | M | `6a43748b8` | WS06-S5 (c) |
| WS06-S8 | L3 | M | `6ac954ff1` | WS06-S7 |
| WS06-S9 | L3 | M | `588a60d35` | WS06-S5 (c) |
| WS06-S10 | L3 | M | `23af43690` | WS06-S7, WS06-S8, WS06-S9 |
| WS08-S1 | L4 | M | `9e5da2c82` | P-UC1 |
| WS08-S2 | L1 | L | `4b0c0638b` | WS01-S1, WS09-S1, WS08-S1 (c) |
| WS08-S3 | L1 | L | `aa6ae39ed` | WS08-S2, WS01-S4 |
| WS08-S4 | L7 | L | `71c66aa00` | P-GW1, WS08-S1 (c), WS08-S3 (c), WS01-S5 (c) |
| WS08-S5 | L6 | M | `3b82693af` | P-CX1 |
| WS08-S6 | L8 | L | `6b371a166` | WS01-S6, P-SPA2, P-SPA1, WS08-S4 (c) |
| WS08-S8 | L8 | S | `91c9d7f16` | WS08-S6, P-SPA1, WS08-S5 (c) |
| WS09-S2 | L8 | S | `efe6144f6` | P-SPA2, P-SPA1 |
| WS09-S3 | L2 | M | `7f6f5cdab` | M0-BASE |
| WS09-S4 | L2 | L | `60db2b789` | WS09-S3 |
| WS09-S5 | L8 | M | `261d57710` | P-SPA1, WS09-S4 (c) |
| WS09-S6 | L8 | S | `388f7773e` | WS09-S5, P-SPA1 |
| WS09-S7 | L4 | M | `1b7580092` | P-UC1 |
| WS09-S8 | L2 | M | `46d385127` | WS09-S4, WS09-S7 (c) |
| WS10-S1 | L2 | M | `5334f633f` | M0-BASE |
| WS10-S2 | L4 | M | `e06833dc1` | P-UC1 |
| WS10-S3 | L3 | L | `d2be41cbe` | WS10-S1, WS10-S2 (c) |
| WS10-S4 | L7 | M | `e417f86fc` | P-GW1, WS10-S1 (c), WS10-S2 (c), WS10-S3 (c) |
| WS10-S5a | L4 | M | `cdc3e0b97` | WS10-S2, P-UC1 |
| WS10-S5b | L8 | M | `1049df2d5` | P-SPA1, WS10-S5a (c) |
| WS10-S6 | L2 | L | `ee6a809ca` | WS10-S1, WS10-S3 (c), WS01-S3 (c) |
| WS10-S7 | L2 | S | `f4a2459c9` | WS10-S6, WS01-S3 (c), WS08-S2 (c) |
| WS10-S8 | L3 | L | `419578392` | WS10-S1, WS10-S3, WS10-S6 (c) |
| WS10-S9 | L3 | M | `a8e027718` | WS10-S1, WS10-S3 |
| WS11-S1 | L4 | S | `2bdc7ab6c` | P-UC1 |
| WS11-S2 | L2 | L | `8e4ea3517` | WS15-S5, WS11-S1 (c) |
| WS11-S3 | L6 | L | `53ff1471c` | P-CX1, WS11-S1 (c), WS11-S2 (c) |
| WS11-S4 | L9 | M | `907ee8498` | P-SPA1, WS11-S3 (c) |
| WS11-S5 | L2 | M | `b3d53ca1b` | WS02-S1 |
| WS11-S6 | L6 | L | `c64622ff4` | WS11-S3, P-CX1, WS11-S5 (c) |
| WS11-S7 | L9 | L | `afd46aa77` | P-SPA1, WS11-S2 (c), WS11-S6 (c) |
| WS11-S8 | L7 | S | `db6d2204a` | P-GW1, WS11-S6 (c) |
| WS11-S9 | L6 | M | `74eada311` | WS11-S6, P-CX1 |
| WS11-S10 | L6 | M | `04333da06` | WS11-S3, WS11-S6, P-CX1 |
| WS11-S11 | L6 | M | `594ad15a6` | WS11-S6, P-CX1, WS11-S7 (c) |
| WS12-S1 | L4 | M | `d9bc1e1f0` | P-UC1 |
| WS12-S2 | L6 | M | `9b358eb92` | P-CX1, WS12-S1 (c) |
| WS12-S3 | L7 | L | `b8fbe2ae9` | P-GW1, WS12-S1 (c), WS12-S2 (c), WS01-S5 (c), WS01-S6 (c), WS01-S7b (c) |
| WS12-S4 | L1 | L | `ee2d29fa0` | WS01-S1 |
| WS12-S5 | L4 | M | `90051a719` | P-UC1, WS12-S4 (c) |
| WS12-S6 | L1 | M | `6b6c7bca8` | WS12-S4, WS12-S5 (c) |
| WS12-S7 | L8 | L | `707900c17` | WS01-S7a, P-SPA2, P-SPA1, WS12-S5 (c), WS12-S6 (c), WS01-S7b (c) |
| WS12-S8 | L6 | M | `de627f79d` | P-CX1, WS12-S7 (c), WS12-S4 (c), WS01-S5 (c) |
| WS13-S1 | L1 | L | `fdb3f1d30` | P-SC1 |
| WS13-S2 | L1 | S/M | `733e88f29` | WS13-S1 |
| WS13-S3 | L7 | M | `a1b6eecd0` | P-GW1, WS13-S2 (c) |
| WS13-S4a | L8 | S | `2575e3cda` | WS01-S7a, P-SPA1 |
| WS13-S4b | L8 | S | `da9f3ef78` | WS13-S4a, P-SPA1, WS13-S3 (c) |
| WS13-S5 | L8 | M | `df6c9f625` | WS13-S4a, P-SPA1, WS13-S3 (c) |
| WS13-S6 | L9 | M | `0027817c1` | P-SPA1 |
| WS13-S7 | L8 | S | `c129e5b24` | WS13-S6, P-SPA2, P-SPA1 |
| WS13-S8 | L6 | M | `f7f952452` | P-CX1 |
| WS14-S1 | L4 | M | `b0b3173fa` | P-UC1 |
| WS14-S2 | L4 | L | `34b1f1859` | P-UC1 |
| WS14-S3 | L10 | L | `6ce345f95` | WS14-S1 (c), WS14-S2 (c) |
| WS14-S4 | L10 | M | `2bb0a261a` | WS14-S2 (c) |
| WS14-S5 | L10 | M | `96a1a7020` | WS14-S2 (c) |
| WS14-S6 | L10 | M | `10ef79360` | WS14-S1 (c) |
| WS14-S7 | L7 | M | `cee325c9a` | P-GW1, WS14-S1 (c), WS14-S5 (c), WS14-S6 (c) |
| WS14-S8 | L8 | M | `4d663fd2c` | P-SPA2, P-SPA1 |
| WS14-S9 | L8 | L | `487393d9e` | P-SPA1, WS14-S7 (c) |
| WS15-S1 | L8 | S | `848da02ec` | P-SPA1 |
| WS15-S2 | L8 | S | `db1b2b181` | P-SPA2, P-SPA1 |
| WS15-S3 | L8 | M | `ed2259335` | P-SPA2, P-SPA1 |
| WS15-S4 | L8 | S | `214f4d30a` | WS07-S5, P-SPA1 |
| WS15-S5 | L1 | M | `1080e1be6` | P-SC1 |
| WS15-S6 | L1 | L | `038f6f8fa` | WS15-S5, WS01-S1 (c) |
| WS15-S7 | L7 | M | `43d267ba9` | P-GW1, WS15-S6 (c) |
| WS15-S8 | L8 | L | `e2845e7d1` | WS15-S1, P-SPA1, WS15-S7 (c) |
| WS15-S9 | L1 | M | `2c87529c3` | WS01-S1, WS15-S6 |
| WS15-S11 | L7 | M | `cd78d5684` | P-GW1, WS15-S8 (c), WS15-S9 (c), WS04-S4 (c) |
| WS15-S12 | L6 | M | `06b4a7c8d` | P-CX1, WS15-S11 (c) |
| WS16-S1 | L9 | M | `a7cd2fd18` | M0-BASE |
| WS16-S2 | L4 | S | `9b9a420b9` | P-UC1 |
| WS16-S3 | L9 | M | `37df8cea5` | WS16-S1, P-SPA1 |
| WS16-S4 | L9 | M | `92fcd8eb5` | WS16-S1, P-SPA1 |
| WS16-S5 | L9 | S | `0aecbe69d` | WS16-S3, P-SPA1 |
| WS16-S6 | L7 | S | `e28c4cc5f` | P-GW1 |
| WS16-S7 | L6 | M | `5bbf82022` | P-CX1 |
| WS16-S8 | L7 | M | `2c2d7b871` | P-GW1, WS16-S7 (c), WS01-S5 (c) |
| WS16-S9 | L9 | L | `d75a558cf` | P-SPA1, WS16-S8 (c) |
| WS16-S10 | L9 | M | `39f351603` | WS16-S9, P-SPA1, WS11-S3 (c), WS11-S4 (c) |
| WS16-S11 | L6 | S | `4cdc3a8cd` | WS16-S7, P-CX1 |

## Open items

- Fixed in the commit after batch 1: the SSO settings test handler type and the action registry test id type.
- `quarry-client.ts`: 21 errors that come from the missing Quarry client package in this worktree. Environmental, unchanged.
- Tests: not run in this pass, per the owner's instruction. Batch-1 test additions are unverified until the suites run.
