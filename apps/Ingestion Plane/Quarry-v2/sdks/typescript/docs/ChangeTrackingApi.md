# ChangeTrackingApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**changeCheck**](ChangeTrackingApi.md#changecheckoperation) | **POST** /v1/change/check | Re-fetch a URL and compare against its baseline |
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

