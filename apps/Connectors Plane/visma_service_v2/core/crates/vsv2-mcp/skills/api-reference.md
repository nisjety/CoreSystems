---
id: api-reference
title: Finding the right Visma endpoint and its filter names
description: How to find the Visma ERP API operation for a question with the offline API reference of the Finance, SalesOrders v3 and JournalTx v1 APIs, which catalog name reads it, which filter names it accepts and why an undeclared filter is refused.
tags: [api, reference, endpoints, filters, search, erp, salesorder-v3, journaltx]
tools: [visma_search_api, visma_erp_query, visma_list_endpoints]
primary: [visma_search_api]
prerequisites: [start-here, live-erp-queries]
difficulty: beginner
steps: 4
---
# Finding the right Visma endpoint and its filter names

`visma_search_api` searches Visma's own API references, every operation with the query parameter names it
declares. Three APIs are in it: the Finance ERP API (`erp`), the SalesOrders v3 API (`salesorder_v3`) and the
JournalTx v1 API (`journaltx_v1`). It is offline, so it needs no credentials and calls nothing in Visma. Use it when
the endpoint or a filter name is not known, before `visma_erp_query`, and to see the request shape of a write. The
`api` argument limits a search to one of the three, and each match says which API and host it belongs to.

## 1. Search with the words of the question

```tool-call
visma_search_api
{"query": "customer invoices", "method": "GET"}
```

Each match gives the `method` and `path`, a `summary`, the `query_parameters` it declares and the
`required_query_parameters`. Use the most specific match. Words are matched against the path, the operation id,
the summary, the tags and the parameter names, so a resource name such as customer, supplier or salesorder works
best.

## 2. Read it with visma_erp_query

`catalog_endpoint` names the entry to read with `visma_erp_query`. It is null when no synced endpoint reads that
path: the operation exists in Visma, but this server does not expose it as a read tool. `visma_list_endpoints`
shows which endpoints are synced and what each one needs.

## 3. Filter names

`visma_erp_query` checks the `query` names against the reference for the endpoint, in whichever of the three APIs
it lives. A name the endpoint does not declare is refused, because Visma would ignore it and return the whole
collection. Names are compared without case. Paging names (`page`, `page_size`) do not belong in `query`. Expand switches such as expandPaymentMethods
are declared parameters too.

## 4. What the reference does not cover

It covers those three APIs only, not DataMart and not the warehouse. For those use visma_datamart_schema or
warehouse_describe_table. An endpoint outside the three is not checked up front, so a filter on it is only compared
afterwards (skill `live-erp-queries`). A question that finds nothing should be retried with other words, or without
the `api` limit, before the answer is "not in the API".
