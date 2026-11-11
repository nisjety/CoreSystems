# SchedulesApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**backfillSchedule**](SchedulesApi.md#backfillscheduleoperation) | **POST** /v1/schedules/{schedule_id}/backfill | Backfill missed runs |
| [**createSchedule**](SchedulesApi.md#createscheduleoperation) | **POST** /v1/schedules | Create a cron / one-shot / change-monitor schedule |
| [**deleteSchedule**](SchedulesApi.md#deleteschedule) | **DELETE** /v1/schedules/{schedule_id} | Delete a schedule |
| [**getScheduleRequest**](SchedulesApi.md#getschedulerequest) | **GET** /v1/schedules/{schedule_id}/requests/{request_id} | Read one trigger or backfill request row |
| [**listScheduleRuns**](SchedulesApi.md#listscheduleruns) | **GET** /v1/schedules/{schedule_id}/runs | List the fires of a schedule |
| [**listSchedules**](SchedulesApi.md#listschedules) | **GET** /v1/schedules | List schedules |
| [**pauseSchedule**](SchedulesApi.md#pauseschedule) | **POST** /v1/schedules/{schedule_id}/pause | Pause a schedule |
| [**triggerSchedule**](SchedulesApi.md#triggerschedule) | **POST** /v1/schedules/{schedule_id}/trigger | Trigger an immediate scheduled run |
| [**unpauseSchedule**](SchedulesApi.md#unpauseschedule) | **POST** /v1/schedules/{schedule_id}/unpause | Unpause a schedule |
| [**updateScheduleTag**](SchedulesApi.md#updatescheduletag) | **PATCH** /v1/schedules/{schedule_id} | Set or clear a schedule\&#39;s monitor tag |



## backfillSchedule

> TriggerSchedule202Response backfillSchedule(scheduleId, backfillScheduleRequest, idempotencyKey)

Backfill missed runs

Queues a replay of a past window of a scrape schedule as a request row (WS15.S3a). Only scrape schedules backfill, at most 24 windows, each entirely in the past. The rules and the answers are the trigger\&#39;s, and the outcome is read at GET /v1/schedules/{schedule_id}/requests/{request_id}. With QUARRY_CONTROL__SCHEDULE_REQUESTS off the answer is 501 UNSUPPORTED.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { BackfillScheduleOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
    // BackfillScheduleRequest
    backfillScheduleRequest: ...,
    // string | Same rule as on trigger: forwarded to control in place of a minted key, and an invalid value is ignored. (optional)
    idempotencyKey: idempotencyKey_example,
  } satisfies BackfillScheduleOperationRequest;

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
| **backfillScheduleRequest** | [BackfillScheduleRequest](BackfillScheduleRequest.md) |  | |
| **idempotencyKey** | `string` | Same rule as on trigger: forwarded to control in place of a minted key, and an invalid value is ignored. | [Optional] [Defaults to `undefined`] |

### Return type

[**TriggerSchedule202Response**](TriggerSchedule202Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request row is queued (status pending) |  -  |
| **400** | The body is malformed, end_at is not after start_at, or the window is refused. details.reason is backfill_window_in_future or backfill_too_many_windows when either applies. |  -  |
| **404** | Unknown schedule (also the answer for a schedule of another organization) |  -  |
| **409** | Refused. details.reason is needs_activation, scheduled_jobs_disabled, request_pending or backfill_kind_refused (only a scrape schedule can be backfilled). |  -  |
| **501** | QUARRY_CONTROL__SCHEDULE_REQUESTS is off (UNSUPPORTED) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## createSchedule

> CreateSchedule200Response createSchedule(createScheduleRequest)

Create a cron / one-shot / change-monitor schedule

An executable schedule (scrape, crawl or batch, with a 5-field cron) or a change monitor (preset). With source_id (E4.1-6) an executable crawl runs a source\&#39;s saved job definition: target_ref becomes the source id, and the config is built from the definition as it is stored when the schedule is created, so a later change to the definition does not reach this schedule. Only a crawl is schedulable, and a scrape with no options. Dark: 501 while QUARRY_EDGE__SAVED_JOBS is off. The 400 reasons are on the response.

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
| **400** | Invalid schedule definition. With source_id, details.reason is one of config_conflicts_with_source_id, invalid_source_id, source_id_not_for_monitor, source_has_no_definition, saved_job_kind_mismatch or saved_job_not_schedulable (a batch). source_definition_invalid carries details.fields[]. An option the fire path cannot honour is saved_job_not_schedulable with details.fields[] (code not_schedulable, path definition.options.&lt;key&gt;). |  -  |
| **403** | The source URL is denied by the org web policy (details.reason policy_denied). The schedules ZDR refusal is also a 403 when it is on. |  -  |
| **404** | source_id names no source of this organization, or a deleted one |  -  |
| **501** | source_id is sent while QUARRY_EDGE__SAVED_JOBS is off (UNSUPPORTED; the message names the switch) |  -  |

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


## getScheduleRequest

> GetScheduleRequest200Response getScheduleRequest(scheduleId, requestId)

Read one trigger or backfill request row

Control\&#39;s view of a request row the caller queued (WS15.S3a): the status (pending, claimed, done, failed or rejected), the attempt count, the params, the result and a short error text. The claiming worker and the lease are never shown. A row of another schedule or organization is a 404.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { GetScheduleRequestRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
    // string
    requestId: requestId_example,
  } satisfies GetScheduleRequestRequest;

  try {
    const data = await api.getScheduleRequest(body);
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
| **requestId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**GetScheduleRequest200Response**](GetScheduleRequest200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The request row |  -  |
| **400** | A malformed schedule id or request id (invalid schedule id, invalid request id) |  -  |
| **404** | Unknown request, or a request of another schedule or organization |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listScheduleRuns

> ListScheduleRuns200Response listScheduleRuns(scheduleId, limit, cursor)

List the fires of a schedule

One page of the schedule\&#39;s job rows, newest first (WS15.S3a). Each item is a job as control stores it: id, org_id, kind, status, policy, params, schedule_id, run_id, created_at, origin and, once finished, statistics. next_cursor is empty on the last page.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { ListScheduleRunsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
    // number | Page size, clamped to 1 to 100. (optional)
    limit: 56,
    // string | The next_cursor of the previous page. (optional)
    cursor: cursor_example,
  } satisfies ListScheduleRunsRequest;

  try {
    const data = await api.listScheduleRuns(body);
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
| **limit** | `number` | Page size, clamped to 1 to 100. | [Optional] [Defaults to `25`] |
| **cursor** | `string` | The next_cursor of the previous page. | [Optional] [Defaults to `undefined`] |

### Return type

[**ListScheduleRuns200Response**](ListScheduleRuns200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | One page of the schedule\&#39;s fires |  -  |
| **400** | A malformed schedule id |  -  |
| **404** | Unknown schedule (also the answer for a schedule of another organization) |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listSchedules

> ListSchedules200Response listSchedules(limit, cursor, tag)

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
    // string | Keeps only the schedules with this monitor tag (E3.5-6). The value is trimmed and lowercased first; a value that fails the tag grammar is a 400 with reason `invalid_tag`. A blank value is no filter. (optional)
    tag: tag_example,
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
| **tag** | `string` | Keeps only the schedules with this monitor tag (E3.5-6). The value is trimmed and lowercased first; a value that fails the tag grammar is a 400 with reason &#x60;invalid_tag&#x60;. A blank value is no filter. | [Optional] [Defaults to `undefined`] |

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

> TriggerSchedule202Response triggerSchedule(scheduleId, idempotencyKey)

Trigger an immediate scheduled run

Queues one run of the schedule as a request row (WS15.S3a). The orchestrator applies the row on its next claim, so the run starts later. Read the outcome at GET /v1/schedules/{schedule_id}/requests/{request_id}. With QUARRY_CONTROL__SCHEDULE_REQUESTS off the answer is 501 UNSUPPORTED.

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
    // string | Your key for this trigger, forwarded to control in place of a minted one. At most 128 bytes after trimming; an empty, longer or not visible ASCII value is ignored, as if no key were sent. Keys are unique per organization: a repeat of a key that control already holds is refused with 409 request_pending, not answered with the first 202 again. (optional)
    idempotencyKey: idempotencyKey_example,
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
| **idempotencyKey** | `string` | Your key for this trigger, forwarded to control in place of a minted one. At most 128 bytes after trimming; an empty, longer or not visible ASCII value is ignored, as if no key were sent. Keys are unique per organization: a repeat of a key that control already holds is refused with 409 request_pending, not answered with the first 202 again. | [Optional] [Defaults to `undefined`] |

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
| **202** | The request row is queued (status pending) |  -  |
| **404** | Unknown schedule (also the answer for a schedule of another organization) |  -  |
| **409** | Refused. details.reason is needs_activation (the schedule is paused), scheduled_jobs_disabled, or request_pending (a request is open, or the Idempotency-Key is already held). |  -  |
| **501** | QUARRY_CONTROL__SCHEDULE_REQUESTS is off (UNSUPPORTED) |  -  |

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


## updateScheduleTag

> CreateSource200Response updateScheduleTag(scheduleId, tagPatchRequest)

Set or clear a schedule\&#39;s monitor tag

Sets the tag of a schedule the caller\&#39;s org owns. An empty string clears it. A body that names no tag is a 400 with reason &#x60;tag_required&#x60;; a tag that fails the grammar is a 400 with reason &#x60;invalid_tag&#x60;. The answer is control\&#39;s updated schedule view.

### Example

```ts
import {
  Configuration,
  SchedulesApi,
} from '@quarry/client';
import type { UpdateScheduleTagRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new SchedulesApi();

  const body = {
    // string
    scheduleId: scheduleId_example,
    // TagPatchRequest
    tagPatchRequest: ...,
  } satisfies UpdateScheduleTagRequest;

  try {
    const data = await api.updateScheduleTag(body);
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
| **200** | Updated schedule (control-plane view) |  -  |
| **400** | Malformed schedule id, missing tag or invalid tag |  -  |
| **404** | Unknown schedule |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

