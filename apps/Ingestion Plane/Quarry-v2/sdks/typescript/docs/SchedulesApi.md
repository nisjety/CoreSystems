# SchedulesApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**backfillSchedule**](SchedulesApi.md#backfillschedule) | **POST** /v1/schedules/{schedule_id}/backfill | Backfill missed runs |
| [**createSchedule**](SchedulesApi.md#createscheduleoperation) | **POST** /v1/schedules | Create a cron / one-shot / change-monitor schedule |
| [**deleteSchedule**](SchedulesApi.md#deleteschedule) | **DELETE** /v1/schedules/{schedule_id} | Delete a schedule |
| [**listSchedules**](SchedulesApi.md#listschedules) | **GET** /v1/schedules | List schedules |
| [**pauseSchedule**](SchedulesApi.md#pauseschedule) | **POST** /v1/schedules/{schedule_id}/pause | Pause a schedule |
| [**triggerSchedule**](SchedulesApi.md#triggerschedule) | **POST** /v1/schedules/{schedule_id}/trigger | Trigger an immediate scheduled run |
| [**unpauseSchedule**](SchedulesApi.md#unpauseschedule) | **POST** /v1/schedules/{schedule_id}/unpause | Unpause a schedule |



## backfillSchedule

> backfillSchedule(scheduleId)

Backfill missed runs

Currently a stub — returns 202 with a Temporal-pending note.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { BackfillScheduleRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
  } satisfies BackfillScheduleRequest;

  try {
    const data = await api.backfillSchedule(body);
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
| **scheduleId** | `string` |  | [Defaults to `undefined`] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | Backfill accepted (stub while Temporal SDK pending) |  -  |
| **404** | Unknown schedule |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## createSchedule

> CreateSchedule200Response createSchedule(createScheduleRequest)

Create a cron / one-shot / change-monitor schedule

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { CreateScheduleOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // CreateScheduleRequest
    createScheduleRequest: ...,
  } satisfies CreateScheduleOperationRequest;

  try {
    const data = await api.createSchedule(body);
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
| **createScheduleRequest** | [CreateScheduleRequest](CreateScheduleRequest.md) |  | |

### Return type

[**CreateSchedule200Response**](CreateSchedule200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Created schedule |  -  |
| **400** | Invalid schedule definition |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## deleteSchedule

> DeleteSchedule200Response deleteSchedule(scheduleId)

Delete a schedule

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { DeleteScheduleRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
  } satisfies DeleteScheduleRequest;

  try {
    const data = await api.deleteSchedule(body);
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
| **scheduleId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**DeleteSchedule200Response**](DeleteSchedule200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Deleted |  -  |
| **404** | Unknown schedule |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listSchedules

> ListSchedules200Response listSchedules(limit, cursor)

List schedules

Forwarded to the control plane; vendor-neutral schedule wire shape.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { ListSchedulesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // number (optional)
    limit: 56,
    // string (optional)
    cursor: cursor_example,
  } satisfies ListSchedulesRequest;

  try {
    const data = await api.listSchedules(body);
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

[**ListSchedules200Response**](ListSchedules200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Schedule page |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## pauseSchedule

> pauseSchedule(scheduleId)

Pause a schedule

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { PauseScheduleRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
  } satisfies PauseScheduleRequest;

  try {
    const data = await api.pauseSchedule(body);
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
| **scheduleId** | `string` |  | [Defaults to `undefined`] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Paused |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## triggerSchedule

> TriggerSchedule202Response triggerSchedule(scheduleId)

Trigger an immediate scheduled run

Currently a stub — returns 202 with &#x60;status: trigger-accepted&#x60; and a note that the Temporal SDK is not yet wired. 

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { TriggerScheduleRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
  } satisfies TriggerScheduleRequest;

  try {
    const data = await api.triggerSchedule(body);
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
| **scheduleId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**TriggerSchedule202Response**](TriggerSchedule202Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | Trigger accepted (stub while Temporal SDK pending) |  -  |
| **404** | Unknown schedule |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## unpauseSchedule

> unpauseSchedule(scheduleId)

Unpause a schedule

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { UnpauseScheduleRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
  } satisfies UnpauseScheduleRequest;

  try {
    const data = await api.unpauseSchedule(body);
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
| **scheduleId** | `string` |  | [Defaults to `undefined`] |

### Return type

`void` (Empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Unpaused |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

