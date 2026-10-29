---
id: create-sales-order
title: Creating a sales order in Visma, always on hold
description: How to create a sales order in Visma through the two-step write path, what the plan checks, why every order is created on hold, how to release it afterwards, and what to tell the user before and after.
tags: [write, salesorder, create, hold, salesorder-v3]
tools: [visma_salesorder_create_plan, visma_salesorder_create_execute, visma_search_api, visma_salesorder_get]
primary: [visma_salesorder_create_plan, visma_salesorder_create_execute]
prerequisites: [start-here, write-safety, api-reference]
difficulty: intermediate
steps: 5
---
# Creating a sales order in Visma, always on hold

Creating an order changes Visma, so it is a two-step write. A plan checks the order and shows the exact request.
An execute sends it, and only after a person has approved it. Every order is created **on hold**: the path forces
`status` to `Hold`. Releasing it is a separate approved step, see skill `hold-or-release-sales-order`.

Writes must be enabled on the server, and the operator must allow the tenant and the operation `salesorder.create`.
Otherwise the plan is refused and the reason is given.

## 1. Find the fields

Check the field names with `visma_search_api` (the operation is `POST /api/v3/SalesOrders` in the SalesOrders v3
API), not from memory:

```tool-call
visma_search_api
{"query": "create sales order", "method": "POST", "api": "salesorder_v3"}
```

## 2. Plan the order

This API takes plain values, not the `{"value": ...}` wrappers of the Finance API. The order needs `type` (the order
type, for example `SO`), `customer` with an `id` (or a `gln`), and at least one entry in `orderLines` with an
`inventoryId` and a positive `quantity`. The order number comes from Visma's number series, so `orderId` is refused.
`status` may be left out; `"Open"` is refused. Unknown or misspelt fields are refused by name, at any depth.

```tool-call
visma_salesorder_create_plan
{"order": {"type": "SO", "customer": {"id": "C-1001"}, "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]}}
```

The plan returns a `summary`, the exact `plan` and a `confirm_token`. Nothing has been sent yet.

## 3. Ask the user, then execute

Tell the user the summary (the customer, the lines and that the order is on hold) and ask for approval. Then call
`visma_salesorder_create_execute` with the plan and the token unchanged. In the default mode the client shows the
user the whole order, and only an explicit yes sends it; an order too large to show in full (more than 200 lines, or
a body over about 8,000 characters) is refused, so split it. A plan runs once and expires. The result's
`created_order` gives the new order's `type` and `orderId`, taken from the address Visma returns it at, and
`location` is that address.

## 4. If the outcome is unclear

If the result says the outcome is unknown, do not plan again at once. First check in Visma whether the order was
created, because a second plan could create a duplicate. A server or gateway error (500, 502, 503, 504) or a timeout
to a create counts as unknown, since the order may have been made before the error. `visma_salesorder_get` reads one
order when its type and number are known. A result that says `not sent` means the request never left (for example
the sign-in lacks the write scope): nothing was created and a new plan is safe once that is fixed.

## 5. After the order exists

The new order is on hold and nothing ships or invoices from it. When the user wants it released, plan the release
with `visma_salesorder_status_plan`, see skill `hold-or-release-sales-order`. Changing a field of an existing order
is `visma_salesorder_patch_plan`, see skill `edit-sales-order`. Cancelling, completing and copying orders are not
offered as writes.
