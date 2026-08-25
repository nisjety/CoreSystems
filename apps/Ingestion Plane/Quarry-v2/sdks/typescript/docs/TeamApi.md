# TeamApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**teamActivity**](TeamApi.md#teamactivity) | **GET** /v1/team/activity | Recent activity feed |
| [**teamConcurrency**](TeamApi.md#teamconcurrency) | **GET** /v1/team/concurrency | Per-org concurrency snapshot |
| [**teamCreditUsage**](TeamApi.md#teamcreditusage) | **GET** /v1/team/credit-usage | Team credit usage for a window |
| [**teamQueueStatus**](TeamApi.md#teamqueuestatus) | **GET** /v1/team/queue-status | Aggregate queue snapshot for the org |
| [**teamTokenUsage**](TeamApi.md#teamtokenusage) | **GET** /v1/team/token-usage | Team token usage for a window |



## teamActivity

> TeamActivity200Response teamActivity()

Recent activity feed

### Example

```ts
import {
  Configuration,
  TeamApi,
} from '@quarry/client';
import type { TeamActivityRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new TeamApi();

  try {
    const data = await api.teamActivity();
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

[**TeamActivity200Response**](TeamActivity200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Activity page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## teamConcurrency

> TeamConcurrency200Response teamConcurrency()

Per-org concurrency snapshot

### Example

```ts
import {
  Configuration,
  TeamApi,
} from '@quarry/client';
import type { TeamConcurrencyRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new TeamApi();

  try {
    const data = await api.teamConcurrency();
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

[**TeamConcurrency200Response**](TeamConcurrency200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Concurrency snapshot |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## teamCreditUsage

> TeamCreditUsage200Response teamCreditUsage(period)

Team credit usage for a window

### Example

```ts
import {
  Configuration,
  TeamApi,
} from '@quarry/client';
import type { TeamCreditUsageRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new TeamApi();

  const body = {
    // string | \"today\" | \"7d\" | \"30d\" | YYYY-MM-DD (optional)
    period: period_example,
  } satisfies TeamCreditUsageRequest;

  try {
    const data = await api.teamCreditUsage(body);
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
| **period** | `string` | \&quot;today\&quot; | \&quot;7d\&quot; | \&quot;30d\&quot; | YYYY-MM-DD | [Optional] [Defaults to `&#39;7d&#39;`] |

### Return type

[**TeamCreditUsage200Response**](TeamCreditUsage200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Credit usage window |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## teamQueueStatus

> TeamQueueStatus200Response teamQueueStatus()

Aggregate queue snapshot for the org

### Example

```ts
import {
  Configuration,
  TeamApi,
} from '@quarry/client';
import type { TeamQueueStatusRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new TeamApi();

  try {
    const data = await api.teamQueueStatus();
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

[**TeamQueueStatus200Response**](TeamQueueStatus200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Queue status aggregate |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## teamTokenUsage

> TeamTokenUsage200Response teamTokenUsage(period)

Team token usage for a window

### Example

```ts
import {
  Configuration,
  TeamApi,
} from '@quarry/client';
import type { TeamTokenUsageRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new TeamApi();

  const body = {
    // string (optional)
    period: period_example,
  } satisfies TeamTokenUsageRequest;

  try {
    const data = await api.teamTokenUsage(body);
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
| **period** | `string` |  | [Optional] [Defaults to `&#39;7d&#39;`] |

### Return type

[**TeamTokenUsage200Response**](TeamTokenUsage200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Token usage window |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

