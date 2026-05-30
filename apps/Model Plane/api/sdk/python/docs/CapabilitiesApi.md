# model_plane_sdk.CapabilitiesApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**get_capability**](CapabilitiesApi.md#get_capability) | **GET** /v1/capabilities/{id} | Get a capability by ID
[**list_capabilities**](CapabilitiesApi.md#list_capabilities) | **GET** /v1/capabilities | List registered capabilities


# **get_capability**
> Capability get_capability(id)

Get a capability by ID

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.capability import Capability
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
    api_instance = model_plane_sdk.CapabilitiesApi(api_client)
    id = 'id_example' # str | 

    try:
        # Get a capability by ID
        api_response = api_instance.get_capability(id)
        print("The response of CapabilitiesApi->get_capability:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CapabilitiesApi->get_capability: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **id** | **str**|  | 

### Return type

[**Capability**](Capability.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Capability details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_capabilities**
> ListCapabilities200Response list_capabilities(kind=kind, q=q, after_id=after_id, limit=limit)

List registered capabilities

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.list_capabilities200_response import ListCapabilities200Response
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
    api_instance = model_plane_sdk.CapabilitiesApi(api_client)
    kind = 'kind_example' # str |  (optional)
    q = 'q_example' # str | Full-text search query (optional)
    after_id = 'after_id_example' # str | Cursor for pagination (optional)
    limit = 50 # int |  (optional) (default to 50)

    try:
        # List registered capabilities
        api_response = api_instance.list_capabilities(kind=kind, q=q, after_id=after_id, limit=limit)
        print("The response of CapabilitiesApi->list_capabilities:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CapabilitiesApi->list_capabilities: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **kind** | **str**|  | [optional] 
 **q** | **str**| Full-text search query | [optional] 
 **after_id** | **str**| Cursor for pagination | [optional] 
 **limit** | **int**|  | [optional] [default to 50]

### Return type

[**ListCapabilities200Response**](ListCapabilities200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Capabilities list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

