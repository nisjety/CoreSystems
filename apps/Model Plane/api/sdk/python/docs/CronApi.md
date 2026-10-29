# model_plane_sdk.CronApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**create_cron**](CronApi.md#create_cron) | **POST** /v1/cron | Create a cron job
[**delete_cron**](CronApi.md#delete_cron) | **DELETE** /v1/cron/{id} | Delete a cron job
[**get_cron**](CronApi.md#get_cron) | **GET** /v1/cron/{id} | Get a cron job by ID
[**list_cron**](CronApi.md#list_cron) | **GET** /v1/cron | List cron jobs
[**patch_cron**](CronApi.md#patch_cron) | **PATCH** /v1/cron/{id} | Update a cron job


# **create_cron**
> Dict[str, object] create_cron(request_body)

Create a cron job

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
    api_instance = model_plane_sdk.CronApi(api_client)
    request_body = None # Dict[str, object] | 

    try:
        # Create a cron job
        api_response = api_instance.create_cron(request_body)
        print("The response of CronApi->create_cron:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CronApi->create_cron: %s\n" % e)
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
**200** | Cron created |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **delete_cron**
> Dict[str, object] delete_cron(id)

Delete a cron job

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
    api_instance = model_plane_sdk.CronApi(api_client)
    id = 'id_example' # str | 

    try:
        # Delete a cron job
        api_response = api_instance.delete_cron(id)
        print("The response of CronApi->delete_cron:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CronApi->delete_cron: %s\n" % e)
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
**200** | Cron deleted |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_cron**
> Dict[str, object] get_cron(id)

Get a cron job by ID

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
    api_instance = model_plane_sdk.CronApi(api_client)
    id = 'id_example' # str | 

    try:
        # Get a cron job by ID
        api_response = api_instance.get_cron(id)
        print("The response of CronApi->get_cron:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CronApi->get_cron: %s\n" % e)
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
**200** | Cron details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_cron**
> Dict[str, object] list_cron()

List cron jobs

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
    api_instance = model_plane_sdk.CronApi(api_client)

    try:
        # List cron jobs
        api_response = api_instance.list_cron()
        print("The response of CronApi->list_cron:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CronApi->list_cron: %s\n" % e)
```



### Parameters

This endpoint does not need any parameter.

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
**200** | Cron list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **patch_cron**
> Dict[str, object] patch_cron(id, request_body)

Update a cron job

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
    api_instance = model_plane_sdk.CronApi(api_client)
    id = 'id_example' # str | 
    request_body = None # Dict[str, object] | 

    try:
        # Update a cron job
        api_response = api_instance.patch_cron(id, request_body)
        print("The response of CronApi->patch_cron:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling CronApi->patch_cron: %s\n" % e)
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
**200** | Cron updated |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

