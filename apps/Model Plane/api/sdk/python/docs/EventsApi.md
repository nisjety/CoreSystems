# model_plane_sdk.EventsApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**stream_run_events**](EventsApi.md#stream_run_events) | **GET** /v1/runs/{run_id}/events | Stream orchestration events for a run via SSE


# **stream_run_events**
> OrchestrationEvent stream_run_events(run_id)

Stream orchestration events for a run via SSE

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.orchestration_event import OrchestrationEvent
from model_plane_sdk.rest import ApiException
from pprint import pprint

# Defining the host is optional and defaults to http://localhost:8080
# See configuration.py for a list of all supported configuration parameters.
configuration = model_plane_sdk.Configuration(
    host = "http://localhost:8080"
)

# The client must configure the authentication and authorization parameters
# in accordance with the API server security policy.
# Examples for each auth method are provided below, use the example that
# satisfies your auth use case.

# Configure Bearer authorization (JWT): bearerAuth
configuration = model_plane_sdk.Configuration(
    access_token = os.environ["BEARER_TOKEN"]
)

# Enter a context with an instance of the API client
with model_plane_sdk.ApiClient(configuration) as api_client:
    # Create an instance of the API class
    api_instance = model_plane_sdk.EventsApi(api_client)
    run_id = 'run_id_example' # str | 

    try:
        # Stream orchestration events for a run via SSE
        api_response = api_instance.stream_run_events(run_id)
        print("The response of EventsApi->stream_run_events:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling EventsApi->stream_run_events: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **run_id** | **str**|  | 

### Return type

[**OrchestrationEvent**](OrchestrationEvent.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: text/event-stream

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | SSE stream of orchestration events |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

