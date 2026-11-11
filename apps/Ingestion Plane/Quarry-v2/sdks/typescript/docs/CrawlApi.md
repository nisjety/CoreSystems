# CrawlApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**crawlHandoff**](CrawlApi.md#crawlhandoff) | **POST** /v1/crawl | Start a crawl job |
| [**getCrawlResults**](CrawlApi.md#getcrawlresults) | **GET** /v1/crawl/{job_id}/results | The pages a crawl fetched, with their artifact ids (INNH-12) |



## crawlHandoff

> EnvelopeHandoffAck crawlHandoff(crawlRequest, idempotencyKey)

Start a crawl job

Hands off a crawl request to the orchestrator for BFS execution. Returns a job ID for tracking progress. 

### Example

```ts
import {
  Configuration,
  CrawlApi,
} from '@quarry/client';
import type { CrawlHandoffRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new CrawlApi();

  const body = {
    // CrawlRequest
    crawlRequest: ...,
    // string | A caller-chosen key for POST /v1/batch and POST /v1/crawl. At most 128 bytes after trimming. An empty, longer or not visible ASCII value is ignored, as if no key were sent. Keys are unique per organization (control migration 028). A repeated key answers 200 with the job the first request created (the same job_id), and that repeat is not billed again. With QUARRY_EDGE__BATCH_IDEMPOTENCY on, a batch that reuses a key for a different URL list, ingest flag, webhook or max_parallel (the last two when sent) is a 409 CONFLICT with details.reason idempotency_key_reused. With the flag off, or on a crawl, a reused key re-attaches to the held job whatever the body says. (optional)
    idempotencyKey: idempotencyKey_example,
  } satisfies CrawlHandoffRequest;

  try {
    const data = await api.crawlHandoff(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **crawlRequest** | [CrawlRequest](CrawlRequest.md) |  | |
| **idempotencyKey** | `string` | A caller-chosen key for POST /v1/batch and POST /v1/crawl. At most 128 bytes after trimming. An empty, longer or not visible ASCII value is ignored, as if no key were sent. Keys are unique per organization (control migration 028). A repeated key answers 200 with the job the first request created (the same job_id), and that repeat is not billed again. With QUARRY_EDGE__BATCH_IDEMPOTENCY on, a batch that reuses a key for a different URL list, ingest flag, webhook or max_parallel (the last two when sent) is a 409 CONFLICT with details.reason idempotency_key_reused. With the flag off, or on a crawl, a reused key re-attaches to the held job whatever the body says. | [Optional] [Defaults to `undefined`] |

### Return type

[**EnvelopeHandoffAck**](EnvelopeHandoffAck.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Crawl job accepted |  -  |
| **402** | The organization\&#39;s configured crawl page (quarry.crawl.page) allowance is spent (WS6.D-s; only a source that reports an allowance produces this, the default static source never does). Body is BUDGET_EXHAUSTED with details dimension org_allowance, metric, limit_source entitlement and resumable false. No job is created. |  -  |
| **429** | Backlog full (QUARRY_JOB_ADMISSION on): the organization already has its bound of waiting jobs. Body is RATE_LIMITED with details reason org_backlog_full, limit, queued and retry_after_seconds, and a Retry-After header. Nothing is stored. |  -  |
| **502** | Orchestrator unreachable |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getCrawlResults

> GetCrawlResults200Response getCrawlResults(jobId, afterSeq, limit, latest, format, fields)

The pages a crawl fetched, with their artifact ids (INNH-12)

The pages a crawl fetched, one row for each fetch (INNH-12): the page URL, the fetch metadata and the ids of the stored artifacts. This is an index of the pages fetched, not their content; the page content is not in the response, read it by id with &#x60;GET /v1/artifacts/{id}&#x60; (&#x60;markdown_artifact_id&#x60; is the page as Markdown, &#x60;source_artifact_id&#x60; the HTML or the original document). Failed and blocked pages are not rows: they are on &#x60;/errors&#x60;.  Rows are in the order the pages were fetched. &#x60;after_seq&#x60; pages forward: pass the &#x60;next_seq&#x60; of the last page (also in &#x60;meta.page.next&#x60;). &#x60;next_seq&#x60; is absent on the last page. A URL fetched twice (a retry, an escalation to a browser) has two rows; &#x60;latest&#x3D;true&#x60; returns the latest fetch of each URL instead, still in &#x60;seq&#x60; order. When a job has more than 20 000 fetches, &#x60;latest&#x3D;true&#x60; reads the first 20 000 only and &#x60;scan_capped&#x60; is true.  A row from before this release carries &#x60;url&#x60;, &#x60;status&#x60;, &#x60;title&#x60; and &#x60;fetched_at&#x60; only. The artifact ids are absent when nothing was stored (never under Zero Data Retention). On the in-memory artifact backend the ids stop resolving when the edge restarts.  Off unless the edge runs with &#x60;QUARRY_EDGE__JOB_RESULTS&#x3D;1&#x60; (501 otherwise). 403 for a caller under Zero Data Retention (&#x60;error.details.reason&#x60; is &#x60;job_result_urls&#x60;) and for a service principal. The organization comes from the token. 

### Example

```ts
import {
  Configuration,
  CrawlApi,
} from '@quarry/client';
import type { GetCrawlResultsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new CrawlApi();

  const body = {
    // string
    jobId: jobId_example,
    // number | The `next_seq` of the previous page. (optional)
    afterSeq: 56,
    // number (optional)
    limit: 56,
    // boolean | Keep only the latest fetch of each URL. (optional)
    latest: true,
    // 'json' | 'csv' | 'jsonl' | json (the default) is the envelope. csv and jsonl export the same rows as text, and the 200 body is text/csv or application/x-ndjson (CONTRACTS, Job results, E4.1-4). (optional)
    format: format_example,
    // string | With an export format, a comma-separated projection of the row fields, each named once, in the order given. Without a format it is a 400. (optional)
    fields: fields_example,
  } satisfies GetCrawlResultsRequest;

  try {
    const data = await api.getCrawlResults(body);
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters


| Name | Type | Description  | Notes |
|------------- | ------------- | ------------- | -------------|
| **jobId** | `string` |  | [Defaults to `undefined`] |
| **afterSeq** | `number` | The &#x60;next_seq&#x60; of the previous page. | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |
| **latest** | `boolean` | Keep only the latest fetch of each URL. | [Optional] [Defaults to `false`] |
| **format** | `json`, `csv`, `jsonl` | json (the default) is the envelope. csv and jsonl export the same rows as text, and the 200 body is text/csv or application/x-ndjson (CONTRACTS, Job results, E4.1-4). | [Optional] [Defaults to `&#39;json&#39;`] [Enum: json, csv, jsonl] |
| **fields** | `string` | With an export format, a comma-separated projection of the row fields, each named once, in the order given. Without a format it is a 400. | [Optional] [Defaults to `undefined`] |

### Return type

[**GetCrawlResults200Response**](GetCrawlResults200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`, `text/csv`, `application/x-ndjson`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One page of results. With &#x60;format&#x3D;csv&#x60; or &#x60;format&#x3D;jsonl&#x60; the body is that text instead of the envelope, and the kind and cursor are in the headers. |  * X-Quarry-Result-Kind - Export only (&#x60;format&#x3D;csv&#x60; or &#x60;jsonl&#x60;). What the rows are; &#x60;page_index&#x60; is an index of the fetched pages, not their content. The JSON envelope carries the same value as &#x60;data.kind&#x60;. <br>  * X-Quarry-Next-Seq - Export only. The &#x60;after_seq&#x60; of the next page; absent on the last page. <br>  |
| **400** | A bad &#x60;limit&#x60; (not 1 to 500), &#x60;after_seq&#x60; or &#x60;latest&#x60;. &#x60;error.details.field&#x60; names it. |  -  |
| **403** | The token\&#39;s ZDR floor is set (&#x60;error.details.reason&#x60; is &#x60;job_result_urls&#x60;), or the caller is a service principal. |  -  |
| **404** | Not found for this organization, a foreign id, or a job of another kind. |  -  |
| **501** | Job results are not enabled on this edge (QUARRY_EDGE__JOB_RESULTS). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

