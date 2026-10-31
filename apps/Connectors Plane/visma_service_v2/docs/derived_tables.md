# Derived tables

Some v1 bronze tables are not something a Visma API returns. v1 builds them in its sync code, and
Dataform reads them. v2 lands only what the APIs return, so each of these has a function in
`tooling/src/vsv2_tooling/derived/` that reproduces v1's rule, with tests, and a script that checks
the result against the live v1 table.

## `visma_price_classes`

There is no price class list in the Visma APIs. v1 (`clients/tier1_operational.py`,
`get_price_classes`) builds one from two places:

1. every `priceClassId` on a customer location gives a class with an empty description;
2. every `priceClass` object `{id, description}` on an ERP customer adds the class, or supplies the
   description when the class is known but has none.

| Input | v2 endpoint | Source |
|---|---|---|
| `priceClassId` per location | `customer_locations` | Sales Order API `GET /api/v3/Customers/{customerId}/locations`, one request per customer |
| `priceClass` per customer | `customers` | ERP `GET /v1/customer` |

Function: `vsv2_tooling.derived.price_classes`. Check against v1:
`scripts/verify_derived_price_classes.py` (read-only `bq` queries for the pilot tenant).

## `visma_customer_locations`

Not derived but easy to mistake for `locations`: it is the Sales Order API's per-customer location
list (with `priceClassId`), fetched by the `customer_locations` config through a path-parameter
partition. The ERP `/v1/location` list is the separate `locations` config.
