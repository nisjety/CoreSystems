# DefaultApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**cancelBatch**](DefaultApi.md#cancelbatch) | **DELETE** /v1/batch/{job_id} | Cancel a batch (WS15.L2) |
| [**cancelCrawl**](DefaultApi.md#cancelcrawl) | **DELETE** /v1/crawl/{job_id} | Cancel a crawl (WS15.L2) |
| [**createProfile**](DefaultApi.md#createprofileoperation) | **POST** /v1/profiles/create | Create a named, scoped profile owned by the caller (WS17.P0 guard) |
| [**deleteProfile**](DefaultApi.md#deleteprofile) | **DELETE** /v1/profiles/{profile_id} | Erase a profile. Never refused on the ZDR side (wind-down). |
| [**getBatchErrors**](DefaultApi.md#getbatcherrors) | **GET** /v1/batch/{job_id}/errors | Final page failures and blocked pages of a batch, paged by event seq (WS15.L1) |
| [**getBatchStatus**](DefaultApi.md#getbatchstatus) | **GET** /v1/batch/{job_id} | Status document of a batch (WS15.L1) |
| [**getCrawlErrors**](DefaultApi.md#getcrawlerrors) | **GET** /v1/crawl/{job_id}/errors | Final page failures and blocked pages of a crawl, paged by event seq (WS15.L1) |
| [**getCrawlStatus**](DefaultApi.md#getcrawlstatus) | **GET** /v1/crawl/{job_id} | Status document of a crawl (WS15.L1) |
| [**getJobErrors**](DefaultApi.md#getjoberrors) | **GET** /v1/jobs/{job_id}/errors | Final page failures and blocked pages of any durable job (WS15.L1) |
| [**getJobStatus**](DefaultApi.md#getjobstatus) | **GET** /v1/jobs/{job_id} | Status document of a durable job (WS15.L1) |
| [**getProfile**](DefaultApi.md#getprofile) | **GET** /v1/profiles/{profile_id} | Redacted view of one profile (WS17.P0). No cookie value, no storage value, no IndexedDB value. |
| [**listOngoingBatches**](DefaultApi.md#listongoingbatches) | **GET** /v1/batch/ongoing | Batches still in flight for the organization, oldest first (WS15.L1) |
| [**listOngoingCrawls**](DefaultApi.md#listongoingcrawls) | **GET** /v1/crawl/ongoing | Crawls still in flight for the organization, oldest first (WS15.L1) |
| [**listProfiles**](DefaultApi.md#listprofiles) | **GET** /v1/profiles | Profiles the caller may see (WS17.P0 principal and ZDR guard; WS17.P2 owner filter) |
| [**pauseBatch**](DefaultApi.md#pausebatch) | **POST** /v1/batch/{job_id}/pause | Pause a running batch (WS15.L2) |
| [**pauseCrawl**](DefaultApi.md#pausecrawl) | **POST** /v1/crawl/{job_id}/pause | Pause a running crawl (WS15.L2) |
| [**restoreProbeProfile**](DefaultApi.md#restoreprobeprofile) | **POST** /v1/profiles/{profile_id}/restore_probe | Whether a profile can be restored at this instant. No browser is started (WS17.P0). |
| [**resumeBatch**](DefaultApi.md#resumebatch) | **POST** /v1/batch/{job_id}/resume | Resume a paused batch (WS15.L2) |
| [**resumeCrawl**](DefaultApi.md#resumecrawl) | **POST** /v1/crawl/{job_id}/resume | Resume a paused crawl (WS15.L2) |
| [**saveProfile**](DefaultApi.md#saveprofileoperation) | **POST** /v1/profiles | Save a profile snapshot (the import write path). Needs the quarry:profiles:import scope (WS17.P0). |
| [**updateProfile**](DefaultApi.md#updateprofileoperation) | **PATCH** /v1/profiles/{profile_id} | Rename and/or rescope a profile. Owner only on an org-shared row (WS17.P2). |



## cancelBatch

> object cancelBatch(jobId)

Cancel a batch (WS15.L2)

The batch alias of DELETE /v1/crawl/{job_id}. A crawl id on this path is 404.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { CancelBatchRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies CancelBatchRequest;

  try {
    const data = await api.cancelBatch(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_running or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## cancelCrawl

> object cancelCrawl(jobId)

Cancel a crawl (WS15.L2)

An accepted crawl is cancelled at once; a running one stops within one page and the control loop period (2 s). Org-scoped; service principals are refused.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { CancelCrawlRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies CancelCrawlRequest;

  try {
    const data = await api.cancelCrawl(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_running or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## createProfile

> ProfileSummary createProfile(createProfileRequest)

Create a named, scoped profile owned by the caller (WS17.P0 guard)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { CreateProfileOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // CreateProfileRequest
    createProfileRequest: ...,
  } satisfies CreateProfileOperationRequest;

  try {
    const data = await api.createProfile(body);
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
| **createProfileRequest** | [CreateProfileRequest](CreateProfileRequest.md) |  | |

### Return type

[**ProfileSummary**](ProfileSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The created profile summary. |  -  |
| **400** | Bad request, or the ephemeral scope. |  -  |
| **403** | A service principal, or a ZDR organization with the flag on (action create). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## deleteProfile

> deleteProfile(profileId)

Erase a profile. Never refused on the ZDR side (wind-down).

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { DeleteProfileRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    profileId: profileId_example,
  } satisfies DeleteProfileRequest;

  try {
    const data = await api.deleteProfile(body);
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
| **profileId** | `string` |  | [Defaults to `undefined`] |

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
| **204** | Erased. |  -  |
| **403** | A service principal. |  -  |
| **404** | Not found, or not the owner under enforce. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getBatchErrors

> object getBatchErrors(jobId, afterSeq, limit)

Final page failures and blocked pages of a batch, paged by event seq (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetBatchErrorsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
    // number (optional)
    afterSeq: 56,
    // number (optional)
    limit: 56,
  } satisfies GetBatchErrorsRequest;

  try {
    const data = await api.getBatchErrors(body);
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
| **afterSeq** | `number` |  | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `undefined`] |

### Return type

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | errors, blocked, robots_blocked and next_seq, as for crawls. |  -  |
| **403** | The token\&#39;s ZDR floor is set; error URLs are not returned. |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getBatchStatus

> object getBatchStatus(jobId)

Status document of a batch (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetBatchStatusRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies GetBatchStatusRequest;

  try {
    const data = await api.getBatchStatus(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The batch status document. |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getCrawlErrors

> object getCrawlErrors(jobId, afterSeq, limit)

Final page failures and blocked pages of a crawl, paged by event seq (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetCrawlErrorsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
    // number (optional)
    afterSeq: 56,
    // number (optional)
    limit: 56,
  } satisfies GetCrawlErrorsRequest;

  try {
    const data = await api.getCrawlErrors(body);
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
| **afterSeq** | `number` |  | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `undefined`] |

### Return type

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | errors (final failed or retried pages), blocked (pages a rule blocked), robots_blocked (empty until robots enforcement), next_seq when the page was full. |  -  |
| **403** | The token\&#39;s ZDR floor is set; error URLs are not returned. |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getCrawlStatus

> object getCrawlStatus(jobId)

Status document of a crawl (WS15.L1)

A batch id on this path answers 404 (kind mismatch).

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetCrawlStatusRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies GetCrawlStatusRequest;

  try {
    const data = await api.getCrawlStatus(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The crawl status document. |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getJobErrors

> object getJobErrors(jobId, afterSeq, limit)

Final page failures and blocked pages of any durable job (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetJobErrorsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
    // number (optional)
    afterSeq: 56,
    // number (optional)
    limit: 56,
  } satisfies GetJobErrorsRequest;

  try {
    const data = await api.getJobErrors(body);
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
| **afterSeq** | `number` |  | [Optional] [Defaults to `undefined`] |
| **limit** | `number` |  | [Optional] [Defaults to `undefined`] |

### Return type

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | errors, blocked, robots_blocked and next_seq, as for crawls. |  -  |
| **403** | The token\&#39;s ZDR floor is set; error URLs are not returned. |  -  |
| **404** | Not found for this organization. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getJobStatus

> object getJobStatus(jobId)

Status document of a durable job (WS15.L1)

Org-scoped by the verified token. A job id of another organization, or an id that is not a job, answers 404. The document carries status, phase, origin, counts, total and total_kind, warnings and the control state.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetJobStatusRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies GetJobStatusRequest;

  try {
    const data = await api.getJobStatus(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The job status document. |  -  |
| **404** | Not found for this organization, or not a job id. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## getProfile

> ProfileDetail getProfile(profileId)

Redacted view of one profile (WS17.P0). No cookie value, no storage value, no IndexedDB value.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { GetProfileRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    profileId: profileId_example,
  } satisfies GetProfileRequest;

  try {
    const data = await api.getProfile(body);
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
| **profileId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**ProfileDetail**](ProfileDetail.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The redacted profile. Cookie values are null with value_len set. |  -  |
| **403** | A service principal, or a ZDR organization with the flag on (action read). |  -  |
| **404** | Not found, or not visible to the caller under enforce. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listOngoingBatches

> object listOngoingBatches()

Batches still in flight for the organization, oldest first (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { ListOngoingBatchesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  try {
    const data = await api.listOngoingBatches();
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Job list items of the non-terminal batches. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listOngoingCrawls

> object listOngoingCrawls()

Crawls still in flight for the organization, oldest first (WS15.L1)

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { ListOngoingCrawlsRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  try {
    const data = await api.listOngoingCrawls();
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Job list items (status, origin, phase, stats) of the non-terminal crawls. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## listProfiles

> ProfileList listProfiles()

Profiles the caller may see (WS17.P0 principal and ZDR guard; WS17.P2 owner filter)

A service principal is refused on every profile route. Under a ZDR posture the list stays open (wind-down).

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { ListProfilesRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  try {
    const data = await api.listProfiles();
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

[**ProfileList**](ProfileList.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | Profile summaries. |  -  |
| **403** | A service principal. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## pauseBatch

> object pauseBatch(jobId)

Pause a running batch (WS15.L2)

The batch stops before its next URL. Batch alias of the crawl pause.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { PauseBatchRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies PauseBatchRequest;

  try {
    const data = await api.pauseBatch(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_running or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## pauseCrawl

> object pauseCrawl(jobId)

Pause a running crawl (WS15.L2)

The crawl stops before its next page and reports run_paused once per pause. Requires a running job.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { PauseCrawlRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies PauseCrawlRequest;

  try {
    const data = await api.pauseCrawl(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_running or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## restoreProbeProfile

> ProfileRestoreProbe restoreProbeProfile(profileId, changeCheckRequest)

Whether a profile can be restored at this instant. No browser is started (WS17.P0).

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { RestoreProbeProfileRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    profileId: profileId_example,
    // ChangeCheckRequest
    changeCheckRequest: ...,
  } satisfies RestoreProbeProfileRequest;

  try {
    const data = await api.restoreProbeProfile(body);
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
| **profileId** | `string` |  | [Defaults to `undefined`] |
| **changeCheckRequest** | [ChangeCheckRequest](ChangeCheckRequest.md) |  | |

### Return type

[**ProfileRestoreProbe**](ProfileRestoreProbe.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The restore summary. |  -  |
| **403** | A service principal, or a ZDR organization with the flag on (action probe). |  -  |
| **404** | Not found, or not visible to the caller under enforce. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## resumeBatch

> object resumeBatch(jobId)

Resume a paused batch (WS15.L2)

The batch continues with its next URL. Batch alias of the crawl resume.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { ResumeBatchRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies ResumeBatchRequest;

  try {
    const data = await api.resumeBatch(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_paused or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## resumeCrawl

> object resumeCrawl(jobId)

Resume a paused crawl (WS15.L2)

The crawl continues with its next page and reports run_resumed. Requires a paused job.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { ResumeCrawlRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    jobId: jobId_example,
  } satisfies ResumeCrawlRequest;

  try {
    const data = await api.resumeCrawl(body);
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

**object**

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **202** | The request is recorded and the orchestrator has yet to apply it (control.pending is true). |  -  |
| **200** | The job already reflects the request (an accepted job cancelled at once). |  -  |
| **404** | Not found, a foreign id, or a job of another kind. |  -  |
| **409** | The job ended, or the verb does not fit its state. details.reason is ended, not_paused or not_controllable. |  -  |
| **501** | Job control is not enabled on this edge (QUARRY_EDGE__JOB_CONTROL). |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## saveProfile

> SaveProfile200Response saveProfile(saveProfileRequest)

Save a profile snapshot (the import write path). Needs the quarry:profiles:import scope (WS17.P0).

The request body carries the snapshot (cookie values and storage). The body is never returned. Up to 4 MiB; past it the request answers 413.

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { SaveProfileOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // SaveProfileRequest
    saveProfileRequest: ...,
  } satisfies SaveProfileOperationRequest;

  try {
    const data = await api.saveProfile(body);
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
| **saveProfileRequest** | [SaveProfileRequest](SaveProfileRequest.md) |  | |

### Return type

[**SaveProfile200Response**](SaveProfile200Response.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The saved profile id. An ignored save (stale or tombstoned) still answers 200. |  -  |
| **403** | A service principal; a token without quarry:profiles:import; or a ZDR organization with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on (error details zdr_refusal.action save). |  -  |
| **413** | The body is over 4 MiB. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


## updateProfile

> ProfileSummary updateProfile(profileId, updateProfileRequest)

Rename and/or rescope a profile. Owner only on an org-shared row (WS17.P2).

### Example

```ts
import {
  Configuration,
  DefaultApi,
} from '@quarry/client';
import type { UpdateProfileOperationRequest } from '@quarry/client';

async function example() {
  console.log("🚀 Testing @quarry/client SDK...");
  const api = new DefaultApi();

  const body = {
    // string
    profileId: profileId_example,
    // UpdateProfileRequest
    updateProfileRequest: ...,
  } satisfies UpdateProfileOperationRequest;

  try {
    const data = await api.updateProfile(body);
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
| **profileId** | `string` |  | [Defaults to `undefined`] |
| **updateProfileRequest** | [UpdateProfileRequest](UpdateProfileRequest.md) |  | |

### Return type

[**ProfileSummary**](ProfileSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: `application/json`
- **Accept**: `application/json`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | The updated profile summary. |  -  |
| **400** | Bad request. |  -  |
| **403** | A service principal, or a ZDR organization with the flag on (action update). |  -  |
| **404** | Not found, or not the owner under enforce. |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

