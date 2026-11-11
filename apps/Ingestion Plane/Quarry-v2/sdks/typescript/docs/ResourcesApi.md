# ResourcesApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**createSource**](ResourcesApi.md#createsourceoperation) | **POST** /v1/sources | Register an ingestion source |
| [**deleteSource**](ResourcesApi.md#deletesource) | **DELETE** /v1/sources/{source_id} | Soft-delete an ingestion source |
| [**getArtifact**](ResourcesApi.md#getartifact) | **GET** /v1/artifacts/{artifact_id} | Fetch artifact bytes or metadata |
| [**getJobResults**](ResourcesApi.md#getjobresults) | **GET** /v1/jobs/{job_id}/results | The pages any durable job fetched, with their artifact ids (INNH-12) |
| [**getSource**](ResourcesApi.md#getsource) | **GET** /v1/sources/{source_id} | Read one ingestion source |
| [**listArtifacts**](ResourcesApi.md#listartifacts) | **GET** /v1/artifacts | List stored artifacts (served locally by edge) |
| [**listBatchEvents**](ResourcesApi.md#listbatchevents) | **GET** /v1/batch/{job_id}/events | Job events of a batch job (same contract as /v1/jobs/{job_id}/events) |
| [**listBenchmarks**](ResourcesApi.md#listbenchmarks) | **GET** /v1/benchmarks | List benchmark runs (placeholder) |
| [**listCrawlEvents**](ResourcesApi.md#listcrawlevents) | **GET** /v1/crawl/{job_id}/events | Job events of a crawl job (same contract as /v1/jobs/{job_id}/events) |
| [**listJobEvents**](ResourcesApi.md#listjobevents) | **GET** /v1/jobs/{job_id}/events | Durable events keyed by handoff job_id, or a live tail with follow&#x3D;true |
| [**listJobsByKind**](ResourcesApi.md#listjobsbykind) | **GET** /v1/{kind}/jobs | List jobs of one kind |
| [**listRequestQueues**](ResourcesApi.md#listrequestqueues) | **GET** /v1/request-queues | List request queues with live stats |
| [**listRunEvents**](ResourcesApi.md#listrunevents) | **GET** /v1/runs/{run_id}/events | Durable event history for one run |
| [**listSnapshots**](ResourcesApi.md#listsnapshots) | **GET** /v1/snapshots | List captured page snapshots |
| [**listSources**](ResourcesApi.md#listsources) | **GET** /v1/sources | List ingestion sources |
| [**putSourceDefinition**](ResourcesApi.md#putsourcedefinition) | **PUT** /v1/sources/{source_id}/definition | Replace or clear a source\&#39;s saved job definition |
| [**updateSourceTag**](ResourcesApi.md#updatesourcetag) | **PATCH** /v1/sources/{source_id} | Set or clear a source\&#39;s monitor tag |



## createSource

> CreateSource200Response createSource(createSourceRequest)

Register an ingestion source

config.definition (E4.1-6) is a saved job definition for the source. The edge checks it with the same validators as POST /v1/crawl (crawl) or POST /v1/scrape (scrape) before anything is stored, and control keeps the canonical form. A refused definition is a 400 with details.fields[]. Sending the key at all, null included, needs QUARRY_EDGE__SAVED_JOBS on, else 501. A repeat registration of the same URL returns the row that exists and does not rewrite its config: change a stored definition with PUT /v1/sources/{source_id}/definition.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { CreateSourceOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // CreateSourceRequest
    createSourceRequest: ...,
  } satisfies CreateSourceOperationRequest;

  try {
    const data = await api.createSource(body);
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
| **createSourceRequest** | [CreateSourceRequest](CreateSourceRequest.md) |  | |

### Return type

[**CreateSource200Response**](CreateSource200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Created source (control-plane wire shape) |  -  |
| **400** | Invalid body |  -  |
| **501** | config.definition is sent while QUARRY_EDGE__SAVED_JOBS is off (UNSUPPORTED; the message names the switch) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## deleteSource

> DeleteSource200Response deleteSource(sourceId)

Soft-delete an ingestion source

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { DeleteSourceRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    sourceId: sourceId_example,
  } satisfies DeleteSourceRequest;

  try {
    const data = await api.deleteSource(body);
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
| **sourceId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**DeleteSource200Response**](DeleteSource200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Deletion accepted (untyped control response) |  -  |
| **404** | Unknown source for this org |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getArtifact

> GetArtifact200Response getArtifact(artifactId)

Fetch artifact bytes or metadata

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { GetArtifactRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    artifactId: artifactId_example,
  } satisfies GetArtifactRequest;

  try {
    const data = await api.getArtifact(body);
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
| **artifactId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetArtifact200Response**](GetArtifact200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Artifact bytes or metadata envelope |  -  |
| **404** | Unknown artifact id |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getJobResults

> GetCrawlResults200Response getJobResults(jobId, afterSeq, limit, latest, format, fields)

The pages any durable job fetched, with their artifact ids (INNH-12)

The pages a job fetched, one row for each fetch (INNH-12): the page URL, the fetch metadata and the ids of the stored artifacts. This is an index of the pages fetched, not their content; the page content is not in the response, read it by id with &#x60;GET /v1/artifacts/{id}&#x60; (&#x60;markdown_artifact_id&#x60; is the page as Markdown, &#x60;source_artifact_id&#x60; the HTML or the original document). Failed and blocked pages are not rows: they are on &#x60;/errors&#x60;.  Rows are in the order the pages were fetched. &#x60;after_seq&#x60; pages forward: pass the &#x60;next_seq&#x60; of the last page (also in &#x60;meta.page.next&#x60;). &#x60;next_seq&#x60; is absent on the last page. A URL fetched twice (a retry, an escalation to a browser) has two rows; &#x60;latest&#x3D;true&#x60; returns the latest fetch of each URL instead, still in &#x60;seq&#x60; order. When a job has more than 20 000 fetches, &#x60;latest&#x3D;true&#x60; reads the first 20 000 only and &#x60;scan_capped&#x60; is true.  A row from before this release carries &#x60;url&#x60;, &#x60;status&#x60;, &#x60;title&#x60; and &#x60;fetched_at&#x60; only. The artifact ids are absent when nothing was stored (never under Zero Data Retention). On the in-memory artifact backend the ids stop resolving when the edge restarts.  Off unless the edge runs with &#x60;QUARRY_EDGE__JOB_RESULTS&#x3D;1&#x60; (501 otherwise). 403 for a caller under Zero Data Retention (&#x60;error.details.reason&#x60; is &#x60;job_result_urls&#x60;) and for a service principal. The organization comes from the token. 

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { GetJobResultsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

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
  } satisfies GetJobResultsRequest;

  try {
    const data = await api.getJobResults(body);
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


## getSource

> GetSource200Response getSource(sourceId)

Read one ingestion source

The source of the caller\&#39;s organization, config included: a saved job definition sits at config.definition (E4.1-6). This route reads what is stored, so it is not behind QUARRY_EDGE__SAVED_JOBS. A malformed id is a 400; a missing, deleted or other organization\&#39;s id is a 404.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { GetSourceRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    sourceId: sourceId_example,
  } satisfies GetSourceRequest;

  try {
    const data = await api.getSource(body);
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
| **sourceId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetSource200Response**](GetSource200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The source (control-plane wire shape) |  -  |
| **400** | Malformed source id |  -  |
| **404** | Unknown source for this org |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listArtifacts

> ListArtifacts200Response listArtifacts(limit, cursor, kind, runId)

List stored artifacts (served locally by edge)

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListArtifactsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // number | Page size (optional)
    limit: 56,
    // string | Opaque cursor from a prior page (optional)
    cursor: cursor_example,
    // 'markdown' | 'html' | 'raw' | 'pdf' | 'screenshot' | 'extract' | Keep only artifacts of this kind. Any other value is a 400 (`error.details.field` is `kind`). (optional)
    kind: kind_example,
    // string | Keep only the artifacts one run wrote (`run_...`, as `ArtifactSummary.run_id` and the per-job results carry it). Any other value is a 400 (`error.details.field` is `run_id`). The filter works on the in-memory and filesystem backends; an S3 backend cannot list, so it lists nothing. (optional)
    runId: runId_example,
  } satisfies ListArtifactsRequest;

  try {
    const data = await api.listArtifacts(body);
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
| **limit** | `number` | Page size | [Optional] [Defaults to `50`] |
| **cursor** | `string` | Opaque cursor from a prior page | [Optional] [Defaults to `undefined`] |
| **kind** | `markdown`, `html`, `raw`, `pdf`, `screenshot`, `extract` | Keep only artifacts of this kind. Any other value is a 400 (&#x60;error.details.field&#x60; is &#x60;kind&#x60;). | [Optional] [Defaults to `undefined`] [Enum: markdown, html, raw, pdf, screenshot, extract] |
| **runId** | `string` | Keep only the artifacts one run wrote (&#x60;run_...&#x60;, as &#x60;ArtifactSummary.run_id&#x60; and the per-job results carry it). Any other value is a 400 (&#x60;error.details.field&#x60; is &#x60;run_id&#x60;). The filter works on the in-memory and filesystem backends; an S3 backend cannot list, so it lists nothing. | [Optional] [Defaults to `undefined`] |

### Return type

[**ListArtifacts200Response**](ListArtifacts200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Artifact page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listBatchEvents

> string listBatchEvents(jobId, follow, replay)

Job events of a batch job (same contract as /v1/jobs/{job_id}/events)

A crawl id on this path is 404. The follow, replay, after_seq, limit and Last-Event-ID rules are those of list_job_events.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListBatchEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    jobId: jobId_example,
    // string (optional)
    follow: follow_example,
    // 'all' | 'none' (optional)
    replay: replay_example,
  } satisfies ListBatchEventsRequest;

  try {
    const data = await api.listBatchEvents(body);
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
| **follow** | `string` |  | [Optional] [Defaults to `undefined`] |
| **replay** | `all`, `none` |  | [Optional] [Defaults to `undefined`] [Enum: all, none] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE re-emission (legacy drain or live tail) |  -  |
| **404** | Foreign id, or a job that is not a batch. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listBenchmarks

> ListBenchmarks200Response listBenchmarks()

List benchmark runs (placeholder)

Live benchmark corpus placeholder — currently returns an empty page until the benchmark corpus ships. 

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListBenchmarksRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  try {
    const data = await api.listBenchmarks();
    console.log(data);
  } catch (error) {
    console.error(error);
  }
}

// Run the test
example().catch(console.error);
```

### Parameters

This endpoint does not need any parameter.

### Return type

[**ListBenchmarks200Response**](ListBenchmarks200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Benchmark page (empty placeholder today) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listCrawlEvents

> string listCrawlEvents(jobId, follow, replay)

Job events of a crawl job (same contract as /v1/jobs/{job_id}/events)

A batch id on this path is 404. The follow, replay, after_seq, limit and Last-Event-ID rules are those of list_job_events.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListCrawlEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    jobId: jobId_example,
    // string (optional)
    follow: follow_example,
    // 'all' | 'none' (optional)
    replay: replay_example,
  } satisfies ListCrawlEventsRequest;

  try {
    const data = await api.listCrawlEvents(body);
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
| **follow** | `string` |  | [Optional] [Defaults to `undefined`] |
| **replay** | `all`, `none` |  | [Optional] [Defaults to `undefined`] [Enum: all, none] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE re-emission (legacy drain or live tail) |  -  |
| **404** | Foreign id, or a job that is not a crawl. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listJobEvents

> string listJobEvents(jobId, follow, replay, afterSeq, limit, lastEventID)

Durable events keyed by handoff job_id, or a live tail with follow&#x3D;true

Without follow (the legacy drain): every event of the job after after_seq (or Last-Event-ID), read page by page until caught up, then a terminal \&quot;done\&quot; frame. With follow&#x3D;true and the edge flag QUARRY_EDGE__JOB_STREAM_FOLLOW&#x3D;1: a live tail. The first frame is \&quot;event: snapshot\&quot; (status, phase, counts, last_seq, deadline_at, origin, control; its id is the start cursor). Every event frame carries \&quot;id: &lt;seq&gt;\&quot; and \&quot;event: &lt;type&gt;\&quot;. The stream ends with \&quot;data: done\&quot; after a terminal event (run_completed, run_failed, run_cancelled, run_timed_out) or when the job record is terminal and the log is caught up. After QUARRY_EDGE__JOB_STREAM_MAX_AGE_S (default 1800) it ends with \&quot;event: reconnect\&quot;; a failed control read ends it with \&quot;event: error\&quot;. Over the per-org or per-instance cap of open streams: 429 RATE_LIMITED with retry_after_seconds. A foreign id is 404 before any frame.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListJobEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    jobId: jobId_example,
    // string | `true` or `1` keeps the stream open. Ignored while the edge flag is off. (optional)
    follow: follow_example,
    // 'all' | 'none' | `all` (default) starts from the first event; `none` starts at the snapshot. Other values are 400. Follow only. (optional)
    replay: replay_example,
    // number | Events after this seq. A Last-Event-ID header wins over it. (optional)
    afterSeq: 56,
    // number | Page size of the legacy drain (default 500, at most 1000). (optional)
    limit: 56,
    // number | Resume after this seq (follow and legacy). (optional)
    lastEventID: 56,
  } satisfies ListJobEventsRequest;

  try {
    const data = await api.listJobEvents(body);
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
| **follow** | `string` | &#x60;true&#x60; or &#x60;1&#x60; keeps the stream open. Ignored while the edge flag is off. | [Optional] [Defaults to `undefined`] |
| **replay** | `all`, `none` | &#x60;all&#x60; (default) starts from the first event; &#x60;none&#x60; starts at the snapshot. Other values are 400. Follow only. | [Optional] [Defaults to `undefined`] [Enum: all, none] |
| **afterSeq** | `number` | Events after this seq. A Last-Event-ID header wins over it. | [Optional] [Defaults to `undefined`] |
| **limit** | `number` | Page size of the legacy drain (default 500, at most 1000). | [Optional] [Defaults to `undefined`] |
| **lastEventID** | `number` | Resume after this seq (follow and legacy). | [Optional] [Defaults to `undefined`] |

### Return type

**string**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE re-emission (legacy drain or live tail) |  -  |
| **404** | Foreign id, or a job of another kind on the crawl and batch paths. |  -  |
| **429** | Over the per-org or per-instance cap of open streams (follow only). Body carries code RATE_LIMITED and retry_after_seconds. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listJobsByKind

> ListJobsByKind200Response listJobsByKind(kind, orgId, limit, cursor)

List jobs of one kind

Forwarded to the control plane. &#x60;kind&#x60; is validated server-side; unknown segments 404.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListJobsByKindRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // 'crawl' | 'search' | 'extract' | 'research' | 'agent' | 'batch' | 'scrape'
    kind: kind_example,
    // string
    orgId: orgId_example,
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
  } satisfies ListJobsByKindRequest;

  try {
    const data = await api.listJobsByKind(body);
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
| **kind** | `crawl`, `search`, `extract`, `research`, `agent`, `batch`, `scrape` |  | [Defaults to `undefined`] [Enum: crawl, search, extract, research, agent, batch, scrape] |
| **orgId** | `string` |  | [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `25`] |
| **cursor** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListJobsByKind200Response**](ListJobsByKind200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Job page |  -  |
| **400** | Missing org_id |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listRequestQueues

> ListRequestQueues200Response listRequestQueues(limit, cursor)

List request queues with live stats

Forwarded to the control plane; aggregates over quarry_queue_items.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListRequestQueuesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
  } satisfies ListRequestQueuesRequest;

  try {
    const data = await api.listRequestQueues(body);
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
| **limit** | `number` |  | [Optional] [Defaults to `50`] |
| **cursor** | `string` |  | [Optional] [Defaults to `undefined`] |

### Return type

[**ListRequestQueues200Response**](ListRequestQueues200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Request queue page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listRunEvents

> ListRunEvents200Response listRunEvents(runId, afterSeq, limit)

Durable event history for one run

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListRunEventsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string | Temporal run id (job_summary.run_id), never job_id
    runId: runId_example,
    // number | Return events strictly after this seq (gap recovery) (optional)
    afterSeq: 56,
    // number (optional)
    limit: 56,
  } satisfies ListRunEventsRequest;

  try {
    const data = await api.listRunEvents(body);
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
| **runId** | `string` | Temporal run id (job_summary.run_id), never job_id | [Defaults to `undefined`] |
| **afterSeq** | `number` | Return events strictly after this seq (gap recovery) | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `100`] |

### Return type

[**ListRunEvents200Response**](ListRunEvents200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Ordered job-history page |  -  |
| **400** | Invalid run_id |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listSnapshots

> ListSnapshots200Response listSnapshots(limit, cursor, url)

List captured page snapshots

Forwarded to the control plane.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListSnapshotsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
    // string | Filter to one tracked URL (optional)
    url: url_example,
  } satisfies ListSnapshotsRequest;

  try {
    const data = await api.listSnapshots(body);
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
| **limit** | `number` |  | [Optional] [Defaults to `50`] |
| **cursor** | `string` |  | [Optional] [Defaults to `undefined`] |
| **url** | `string` | Filter to one tracked URL | [Optional] [Defaults to `undefined`] |

### Return type

[**ListSnapshots200Response**](ListSnapshots200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Snapshot page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listSources

> ListSources200Response listSources(limit, cursor, tag)

List ingestion sources

Forwarded to the control plane; durable per-org resource.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { ListSourcesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
    // string | Keeps only the sources with this monitor tag (E3.5-6). The value is trimmed and lowercased first; a value that fails the tag grammar is a 400 with reason `invalid_tag`. A blank value is no filter. (optional)
    tag: tag_example,
  } satisfies ListSourcesRequest;

  try {
    const data = await api.listSources(body);
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
| **limit** | `number` |  | [Optional] [Defaults to `50`] |
| **cursor** | `string` |  | [Optional] [Defaults to `undefined`] |
| **tag** | `string` | Keeps only the sources with this monitor tag (E3.5-6). The value is trimmed and lowercased first; a value that fails the tag grammar is a 400 with reason &#x60;invalid_tag&#x60;. A blank value is no filter. | [Optional] [Defaults to `undefined`] |

### Return type

[**ListSources200Response**](ListSources200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Source page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## putSourceDefinition

> CreateSource200Response putSourceDefinition(sourceId, sourceDefinitionRequest)

Replace or clear a source\&#39;s saved job definition

Replaces config.definition of a source the caller\&#39;s organization owns, or clears it with &#x60;definition: null&#x60; (E4.1-6). Dark: 501 while QUARRY_EDGE__SAVED_JOBS is off, null included. A definition is checked against the source\&#39;s kind and URL with the validators of POST /v1/crawl (crawl) or POST /v1/scrape (scrape), and the canonical form is stored; the other keys of config are kept. The URL must pass the org web policy: a denied URL is a 403 with details.reason policy_denied. Clearing fetches nothing, so the policy does not apply to it. The answer is the updated source.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { PutSourceDefinitionRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    sourceId: sourceId_example,
    // SourceDefinitionRequest
    sourceDefinitionRequest: ...,
  } satisfies PutSourceDefinitionRequest;

  try {
    const data = await api.putSourceDefinition(body);
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
| **sourceId** | `string` |  | [Defaults to `undefined`] |
| **sourceDefinitionRequest** | [SourceDefinitionRequest](SourceDefinitionRequest.md) |  | |

### Return type

[**CreateSource200Response**](CreateSource200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Updated source (control-plane wire shape) |  -  |
| **400** | Malformed source id; a body that names no definition (details.fields[] with code required); or a definition the validators refuse (details.fields[] with path, code and message, as on SavedJobDefinition) |  -  |
| **403** | The source URL is denied by the org web policy (details.reason policy_denied) |  -  |
| **404** | Unknown source for this org |  -  |
| **501** | QUARRY_EDGE__SAVED_JOBS is off (UNSUPPORTED; the message names the switch) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## updateSourceTag

> CreateSource200Response updateSourceTag(sourceId, tagPatchRequest)

Set or clear a source\&#39;s monitor tag

Sets the tag of a source the caller\&#39;s org owns. An empty string clears it. A body that names no tag is a 400 with reason &#x60;tag_required&#x60;; a tag that fails the grammar is a 400 with reason &#x60;invalid_tag&#x60;. The answer is the updated source in control\&#39;s wire shape.

### Example

```ts
import {
  Configuration,
  ResourcesApi,
} from '@quarry/client';
import type { UpdateSourceTagRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ResourcesApi();

  const body = {
    // string
    sourceId: sourceId_example,
    // TagPatchRequest
    tagPatchRequest: ...,
  } satisfies UpdateSourceTagRequest;

  try {
    const data = await api.updateSourceTag(body);
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
| **sourceId** | `string` |  | [Defaults to `undefined`] |
| **tagPatchRequest** | [TagPatchRequest](TagPatchRequest.md) |  | |

### Return type

[**CreateSource200Response**](CreateSource200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Updated source (control-plane wire shape) |  -  |
| **400** | Malformed source id, missing tag or invalid tag |  -  |
| **404** | Unknown source for this org |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

