---
id: hold-or-release-sales-order
title: Putting a sales order on hold or releasing it
description: How to put an open sales order on hold or release a held one through the two-step write path, which moves are offered, what the plan checks, and what to tell the user before and after.
tags: [write, salesorder, hold, release, status, salesorder-v3]
tools: [visma_salesorder_status_plan, visma_salesorder_status_execute, visma_salesorder_get]
primary: [visma_salesorder_status_plan, visma_salesorder_status_execute]
prerequisites: [start-here, write-safety, sales-orders-live]
difficulty: intermediate
steps: 5
---
# Putting a sales order on hold or releasing it

A hold stops an order from being picked, shipped and invoiced, and a release lets it continue. Both change a real
company's books, so they are a two-step write like every other: a plan builds the exact request, an execute sends it
after a person has approved it. Do it only when the user asks for that specific order.

Writes must be enabled on the server, and the operator must allow the tenant and the operation
`salesorder.set_status`. Otherwise the plan is refused and the reason is given.

## 1. Read the order

The plan reads the order itself, but the user should hear what state it is in first. `status` is an object:
`{"description": "Open", "cancelled": false, "hold": false}`, and a held order is `"On Hold"` with `hold` true.

```tool-call
visma_salesorder_get
{"type": "SO", "orderId": "000100"}
```

## 2. Plan the move

Only two moves exist, and each starts from one state:

| Move | `status` | Only when the order is |
|---|---|---|
| Put on hold | `"Hold"` | `Open` |
| Release | `"Open"` | `On Hold` |

Anything else is refused with the reason: an order that is already on hold, a credit hold or an approval hold (those
have their own owner in Visma), a back order, and every completed or cancelled order. Cancelling, completing and
back-ordering are not offered at all.

```tool-call
visma_salesorder_status_plan
{"type": "SO", "orderId": "000100", "status": "Hold"}
```

To release, send the same call with `"status": "Open"`. The plan reads the order's current version and returns the
`summary`, the exact `plan` and a `confirm_token`. Nothing has been sent yet.

## 3. Ask the user, then execute

Tell the user the summary and ask for approval. Release has consequences, because the order can then be shipped and
invoiced, so say that when the user asks for it. Then call `visma_salesorder_status_execute` with the plan and the
token unchanged. In the default mode the client shows the exact request, and only an explicit yes sends it. A plan
runs once and expires after 10 minutes.

## 4. Possible outcomes

| Result | Meaning | What to do |
|---|---|---|
| `executed: true`, `http_status` 200 or 204 | Visma accepted the change | Verify (step 5) and report |
| not executed: the user did not approve | Nothing was written | Say so |
| rejected with 412 | Someone changed the order after the plan | Read the order again and make a new plan |
| outcome unknown | The request may or may not have reached Visma | **Do not plan again.** Read the order and see |

## 5. Verify

Read the order again and report its status in the user's words, with the company, the order and the `plan_id`.

```tool-call
visma_salesorder_get
{"type": "SO", "orderId": "000100"}
```
