# model_plane_sdk.MemoryApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**create_memory**](MemoryApi.md#create_memory) | **POST** /v1/memory | Create a memory entry
[**delete_memory**](MemoryApi.md#delete_memory) | **DELETE** /v1/memory/{id} | Delete a memory entry
[**get_memory**](MemoryApi.md#get_memory) | **GET** /v1/memory/{id} | Get a memory entry by ID
[**list_memory**](MemoryApi.md#list_memory) | **GET** /v1/memory | List memory entries
[**patch_memory**](MemoryApi.md#patch_memory) | **PATCH** /v1/memory/{id} | Update a memory entry


# **create_memory**
> Dict[str, object] create_memory(request_body)

Create a memory entry

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
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
    api_instance = model_plane_sdk.MemoryApi(api_client)
    request_body = None # Dict[str, object] | 

    try:
        # Create a memory entry
        api_response = api_instance.create_memory(request_body)
        print("The response of MemoryApi->create_memory:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MemoryApi->create_memory: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **request_body** | [**Dict[str, object]**](object.md)|  | 

### Return type

**Dict[str, object]**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Memory created |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **delete_memory**
> Dict[str, object] delete_memory(id)

Delete a memory entry

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
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
    api_instance = model_plane_sdk.MemoryApi(api_client)
    id = 'id_example' # str | 

    try:
        # Delete a memory entry
        api_response = api_instance.delete_memory(id)
        print("The response of MemoryApi->delete_memory:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MemoryApi->delete_memory: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **id** | **str**|  | 

### Return type

**Dict[str, object]**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Memory deleted |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_memory**
> Dict[str, object] get_memory(id)

Get a memory entry by ID

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
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
    api_instance = model_plane_sdk.MemoryApi(api_client)
    id = 'id_example' # str | 

    try:
        # Get a memory entry by ID
        api_response = api_instance.get_memory(id)
        print("The response of MemoryApi->get_memory:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MemoryApi->get_memory: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **id** | **str**|  | 

### Return type

**Dict[str, object]**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Memory details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_memory**
> Dict[str, object] list_memory(scope=scope)

List memory entries

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
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
    api_instance = model_plane_sdk.MemoryApi(api_client)
    scope = 'scope_example' # str |  (optional)

    try:
        # List memory entries
        api_response = api_instance.list_memory(scope=scope)
        print("The response of MemoryApi->list_memory:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MemoryApi->list_memory: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **scope** | **str**|  | [optional] 

### Return type

**Dict[str, object]**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Memory list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **patch_memory**
> Dict[str, object] patch_memory(id, request_body)

Update a memory entry

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
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
    api_instance = model_plane_sdk.MemoryApi(api_client)
    id = 'id_example' # str | 
    request_body = None # Dict[str, object] | 

    try:
        # Update a memory entry
        api_response = api_instance.patch_memory(id, request_body)
        print("The response of MemoryApi->patch_memory:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling MemoryApi->patch_memory: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **id** | **str**|  | 
 **request_body** | [**Dict[str, object]**](object.md)|  | 

### Return type

**Dict[str, object]**

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Memory updated |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

