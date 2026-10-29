# Phase 0 — DataMart probe

- tenant_id: `33f319b9-4d57-11ee-960c-025417856183`
- endpoint:  `https://datamart.visma.net/graphql`

## `accounts` — 500/page × target 1 pages

| page | items | ms | error |
|---|---:|---:|---|
| 1 | 500 | 455 |  |

_total: 455 ms · 1098.9 records/s_

## `customers` — 500/page × target 1 pages

| page | items | ms | error |
|---|---:|---:|---|
| 1 | 500 | 186 |  |

_total: 186 ms · 2688.2 records/s_

## Recommended initial budget

- per-minute: 60 _(tune if probe shows headroom)_
- per-day:    10 000
