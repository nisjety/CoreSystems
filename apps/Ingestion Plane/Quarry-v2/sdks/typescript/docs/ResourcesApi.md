# ResourcesApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**createSource**](ResourcesApi.md#createsourceoperation) | **POST** /v1/sources | Register an ingestion source |
| [**deleteSource**](ResourcesApi.md#deletesource) | **DELETE** /v1/sources/{source_id} | Soft-delete an ingestion source |
| [**getArtifact**](ResourcesApi.md#getartifact) | **GET** /v1/artifacts/{artifact_id} | Fetch artifact bytes or metadata |
| [**listArtifacts**](ResourcesApi.md#listartifacts) | **GET** /v1/artifacts | List stored artifacts (served locally by edge) |
| [**listBenchmarks**](ResourcesApi.md#listbenchmarks) | **GET** /v1/benchmarks | List benchmark runs (placeholder) |
| [**listJobEvents**](ResourcesApi.md#listjobevents) | **GET** /v1/jobs/{job_id}/events | Durable events keyed by handoff job_id |
| [**listJobsByKind**](ResourcesApi.md#listjobsbykind) | **GET** /v1/{kind}/jobs | List jobs of one kind |
| [**listRequestQueues**](ResourcesApi.md#listrequestqueues) | **GET** /v1/request-queues | List request queues with live stats |
| [**listRunEvents**](ResourcesApi.md#listrunevents) | **GET** /v1/runs/{run_id}/events | Durable event history for one run |
| [**listSnapshots**](ResourcesApi.md#listsnapshots) | **GET** /v1/snapshots | List captured page snapshots |
| [**listSources**](ResourcesApi.md#listsources) | **GET** /v1/sources | List ingestion sources |



## createSource

> CreateSource200Response createSource(createSourceRequest)

Register an ingestion source

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


## listArtifacts

> ListArtifacts200Response listArtifacts(limit, cursor, kind)

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
    // 'markdown' | 'html' | 'raw' | 'pdf' | 'screenshot' | 'extract' (optional)
    kind: kind_example,
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
| **kind** | `markdown`, `html`, `raw`, `pdf`, `screenshot`, `extract` |  | [Optional] [Defaults to `undefined`] [Enum: markdown, html, raw, pdf, screenshot, extract] |

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


## listJobEvents

> Array&lt;JobHistoryEvent&gt; listJobEvents(jobId)

Durable events keyed by handoff job_id

Forwards to control\&#39;s /v1/jobs/{id}/events and re-emits as JSON (or SSE for the SPA gateway).

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

### Return type

[**Array&lt;JobHistoryEvent&gt;**](JobHistoryEvent.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`, `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | JSON event array or SSE re-emission |  -  |

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

> ListSources200Response listSources(limit, cursor)

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

