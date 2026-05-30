# model_plane_sdk.InvokeApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**invoke**](InvokeApi.md#invoke) | **POST** /v1/invoke | Synchronous inference request
[**invoke_stream**](InvokeApi.md#invoke_stream) | **POST** /v1/invoke/stream | Streaming inference via SSE


# **invoke**
> InvokeResponse invoke(invoke_request)

Synchronous inference request

Sends content to the configured model via inference-core and returns the
full response. Budget is checked pre-flight against cost-core. A session
run is created in session-core for context tracking.


### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.invoke_request import InvokeRequest
from model_plane_sdk.models.invoke_response import InvokeResponse
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
    api_instance = model_plane_sdk.InvokeApi(api_client)
    invoke_request = model_plane_sdk.InvokeRequest() # InvokeRequest | 

    try:
        # Synchronous inference request
        api_response = api_instance.invoke(invoke_request)
        print("The response of InvokeApi->invoke:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling InvokeApi->invoke: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **invoke_request** | [**InvokeRequest**](InvokeRequest.md)|  | 

### Return type

[**InvokeResponse**](InvokeResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Inference completed |  -  |
**400** | Invalid request |  -  |
**401** | Missing or invalid bearer token |  -  |
**402** | Budget exceeded |  -  |
**502** | Upstream service error |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **invoke_stream**
> SseChunk invoke_stream(invoke_request)

Streaming inference via SSE

Same as /v1/invoke but returns Server-Sent Events. Each event carries a
delta chunk. The final event has `done: true`. Event type `done` signals
end of stream.


### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.invoke_request import InvokeRequest
from model_plane_sdk.models.sse_chunk import SseChunk
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
    api_instance = model_plane_sdk.InvokeApi(api_client)
    invoke_request = model_plane_sdk.InvokeRequest() # InvokeRequest | 

    try:
        # Streaming inference via SSE
        api_response = api_instance.invoke_stream(invoke_request)
        print("The response of InvokeApi->invoke_stream:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling InvokeApi->invoke_stream: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **invoke_request** | [**InvokeRequest**](InvokeRequest.md)|  | 

### Return type

[**SseChunk**](SseChunk.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: text/event-stream, application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | SSE stream of inference chunks |  -  |
**400** | Invalid request |  -  |
**401** | Missing or invalid bearer token |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

