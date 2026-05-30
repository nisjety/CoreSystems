# EventsApi

All URIs are relative to *http://localhost:8080*

| Method | HTTP request | Description |
|------------- | ------------- | -------------|
| [**streamRunEvents**](EventsApi.md#streamrunevents) | **GET** /v1/runs/{run_id}/events | Stream orchestration events for a run via SSE |



## streamRunEvents

> OrchestrationEvent streamRunEvents(runId)

Stream orchestration events for a run via SSE

### Example

```ts
import {
  Configuration,
  EventsApi,
} from '@model-plane/sdk';
import type { StreamRunEventsRequest } from '@model-plane/sdk';

async function example() {
  console.log("🚀 Testing @model-plane/sdk SDK...");
  const config = new Configuration({ 
    // Configure HTTP bearer authorization: bearerAuth
    accessToken: "YOUR BEARER TOKEN",
  });
  const api = new EventsApi(config);

  const body = {
    // string
    runId: runId_example,
  } satisfies StreamRunEventsRequest;

  try {
    const data = await api.streamRunEvents(body);
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
| **runId** | `string` |  | [Defaults to `undefined`] |

### Return type

[**OrchestrationEvent**](OrchestrationEvent.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: `text/event-stream`


### HTTP response details
| Status code | Description | Response headers |
|-------------|-------------|------------------|
| **200** | SSE stream of orchestration events |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)

