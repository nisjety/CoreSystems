# ChangeTrackingApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**changeCheck**](ChangeTrackingApi.md#changecheckoperation) | **POST** /v1/change/check | Re-fetch a URL and compare against its baseline |
| [**changeDiff**](ChangeTrackingApi.md#changediff) | **GET** /v1/change/diff | The two versions of a tracked page and the paragraph diff between them (INNH-4) |
| [**changeHistory**](ChangeTrackingApi.md#changehistory) | **GET** /v1/change/history | Ordered baseline chain for a tracked URL |
| [**changeLatest**](ChangeTrackingApi.md#changelatest) | **GET** /v1/change/latest | Most recent baseline for a tracked URL |
| [**promoteTrackedResultToSnapshot**](ChangeTrackingApi.md#promotetrackedresulttosnapshot) | **POST** /v1/change/snapshot | Promote the latest baseline to the public Snapshot shape |
| [**scheduleRefreshRun**](ChangeTrackingApi.md#schedulerefreshrunoperation) | **POST** /v1/change/refresh | Enqueue an immediate re-check onto the durable frontier |



## changeCheck

> ChangeCheck200Response changeCheck(changeCheckRequest)

Re-fetch a URL and compare against its baseline

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { ChangeCheckOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // ChangeCheckRequest
    changeCheckRequest: ...,
  } satisfies ChangeCheckOperationRequest;

  try {
    const data = await api.changeCheck(body);
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
| **changeCheckRequest** | [ChangeCheckRequest](ChangeCheckRequest.md) |  | |

### Return type

[**ChangeCheck200Response**](ChangeCheck200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Change record (new | unchanged | changed | unreachable) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## changeDiff

> ChangeDiff200Response changeDiff(url, from, to, diffId, include)

The two versions of a tracked page and the paragraph diff between them (INNH-4)

Returns the stored text of two baselines of a tracked URL and the paragraph diff between them. A baseline keeps its page text only for a change monitor, only when the edge runs with &#x60;QUARRY_EDGE__CHANGE_TEXT_DIFF&#x60; on, and never under Zero Data Retention.  Pick the pair one way: &#x60;url&#x60; alone compares the latest two baselines; &#x60;url&#x60; with &#x60;from&#x60; and &#x60;to&#x60; compares two named baselines of that url (any two, in the order given); &#x60;diff_id&#x60; reads the diff a &#x60;quarry.change.detected&#x60; event announced (&#x60;url&#x60; may be added and must then be the diff\&#39;s url). &#x60;include&#x3D;markdown&#x60; adds the two pages\&#39; text, each cut at 1 MiB.  The diff is paragraph-level and set-based: a paragraph is added when the newer text has it and the older does not, removed in the opposite case, moved when it kept its text but changed its place among the paragraphs both versions hold (an inserted paragraph does not make the rest move). Whitespace and line endings are ignored. &#x60;real_delta&#x60; is false when nothing was added or removed, which is the honest answer for a page whose raw bytes changed (an ad, a timestamp in the markup) and whose text did not. The diff holds at most 500 ops and 2 KB of text per op; the counts are complete and &#x60;truncated&#x60; says whether anything was cut.  When the pair has no readable text the answer is 200 with &#x60;available: false&#x60; and a &#x60;reason&#x60;: &#x60;no_previous_version&#x60; (the url has one baseline), &#x60;no_previous_text&#x60; (the older baseline was recorded before text was kept), &#x60;no_text&#x60; (the newer one has none) or &#x60;artifact_gone&#x60; (a stored page was evicted or cannot be read). The two sides\&#39; metadata is returned either way.  An id of another organization answers 404 exactly like an id that does not exist. 501 when the edge runs without the flag; 403 for a Zero Data Retention caller. 

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { ChangeDiffRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // string | The tracked URL. Required unless `diff_id` is given. (optional)
    url: url_example,
    // string | The older baseline id (`bln_...`). Needs `to` and `url`. (optional)
    from: from_example,
    // string | The newer baseline id (`bln_...`). Needs `from` and `url`. (optional)
    to: to_example,
    // string | A stored diff id (`diff_...`), as `quarry.change.detected` and `ChangeRecord.diff_id` carry it. Cannot be combined with `from` or `to`. (optional)
    diffId: diffId_example,
    // 'markdown' | `markdown` adds the stored text of both versions. (optional)
    include: include_example,
  } satisfies ChangeDiffRequest;

  try {
    const data = await api.changeDiff(body);
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
| **url** | `string` | The tracked URL. Required unless &#x60;diff_id&#x60; is given. | [Optional] [Defaults to `undefined`] |
| **from** | `string` | The older baseline id (&#x60;bln_...&#x60;). Needs &#x60;to&#x60; and &#x60;url&#x60;. | [Optional] [Defaults to `undefined`] |
| **to** | `string` | The newer baseline id (&#x60;bln_...&#x60;). Needs &#x60;from&#x60; and &#x60;url&#x60;. | [Optional] [Defaults to `undefined`] |
| **diffId** | `string` | A stored diff id (&#x60;diff_...&#x60;), as &#x60;quarry.change.detected&#x60; and &#x60;ChangeRecord.diff_id&#x60; carry it. Cannot be combined with &#x60;from&#x60; or &#x60;to&#x60;. | [Optional] [Defaults to `undefined`] |
| **include** | `markdown` | &#x60;markdown&#x60; adds the stored text of both versions. | [Optional] [Defaults to `undefined`] [Enum: markdown] |

### Return type

[**ChangeDiff200Response**](ChangeDiff200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The two versions and the diff, or why there is no diff |  -  |
| **400** | Neither &#x60;url&#x60; nor &#x60;diff_id&#x60;; &#x60;from&#x60; without &#x60;to&#x60;; &#x60;diff_id&#x60; with &#x60;from&#x60; or &#x60;to&#x60;; baselines of different urls or the same baseline twice; an unknown &#x60;include&#x60;; an id over 128 characters |  -  |
| **403** | The caller is under Zero Data Retention. Stored page text is not readable. |  -  |
| **404** | No baseline for this org and url, or an unknown baseline or diff id, or one that belongs to another organization |  -  |
| **501** | Change text diffs are not enabled on this edge (QUARRY_EDGE__CHANGE_TEXT_DIFF), or the edge has no baseline store |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## changeHistory

> ChangeHistory200Response changeHistory(url, limit)

Ordered baseline chain for a tracked URL

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { ChangeHistoryRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // string
    url: url_example,
    // number (optional)
    limit: 56,
  } satisfies ChangeHistoryRequest;

  try {
    const data = await api.changeHistory(body);
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
| **url** | `string` |  | [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `20`] |

### Return type

[**ChangeHistory200Response**](ChangeHistory200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Baseline chain (most recent first) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## changeLatest

> ChangeLatest200Response changeLatest(url)

Most recent baseline for a tracked URL

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { ChangeLatestRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // string
    url: url_example,
  } satisfies ChangeLatestRequest;

  try {
    const data = await api.changeLatest(body);
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
| **url** | `string` |  | [Defaults to `undefined`] |

### Return type

[**ChangeLatest200Response**](ChangeLatest200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Latest baseline snapshot |  -  |
| **404** | No baseline for this org+URL |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## promoteTrackedResultToSnapshot

> PromoteTrackedResultToSnapshot200Response promoteTrackedResultToSnapshot(url)

Promote the latest baseline to the public Snapshot shape

Requires postgres-queue + DATABASE_URL; 501 with a hint otherwise.

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { PromoteTrackedResultToSnapshotRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // string
    url: url_example,
  } satisfies PromoteTrackedResultToSnapshotRequest;

  try {
    const data = await api.promoteTrackedResultToSnapshot(body);
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
| **url** | `string` |  | [Defaults to `undefined`] |

### Return type

[**PromoteTrackedResultToSnapshot200Response**](PromoteTrackedResultToSnapshot200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Promoted snapshot |  -  |
| **404** | No baseline for this URL/org |  -  |
| **501** | postgres-queue feature disabled |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## scheduleRefreshRun

> ScheduleRefreshRun200Response scheduleRefreshRun(scheduleRefreshRunRequest)

Enqueue an immediate re-check onto the durable frontier

ScheduleRefreshRun — enqueues a &#x60;change_refresh&#x60; request on the org-scoped Postgres queue (SKIP LOCKED bridge). Idempotent enqueue: &#x60;accepted&#x3D;false&#x60; when an identical request was already queued. Requires postgres-queue. 

### Example

```ts
import {
  Configuration,
  ChangeTrackingApi,
} from '@quarry/client';
import type { ScheduleRefreshRunOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new ChangeTrackingApi();

  const body = {
    // ScheduleRefreshRunRequest
    scheduleRefreshRunRequest: ...,
  } satisfies ScheduleRefreshRunOperationRequest;

  try {
    const data = await api.scheduleRefreshRun(body);
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
| **scheduleRefreshRunRequest** | [ScheduleRefreshRunRequest](ScheduleRefreshRunRequest.md) |  | |

### Return type

[**ScheduleRefreshRun200Response**](ScheduleRefreshRun200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Enqueue result |  -  |
| **501** | postgres-queue feature disabled |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

