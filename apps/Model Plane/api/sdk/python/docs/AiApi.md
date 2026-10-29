# model_plane_sdk.AiApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**ai_chat**](AiApi.md#ai_chat) | **POST** /v1/ai/chat | Chat completion via inference-core
[**ai_documents**](AiApi.md#ai_documents) | **POST** /v1/ai/documents | Document processing (queued)
[**ai_images**](AiApi.md#ai_images) | **POST** /v1/ai/images | Image generation (queued)
[**ai_realtime**](AiApi.md#ai_realtime) | **POST** /v1/ai/realtime | Realtime session placeholder
[**ai_speech**](AiApi.md#ai_speech) | **POST** /v1/ai/speech | Text-to-speech synthesis (queued)
[**ai_translate**](AiApi.md#ai_translate) | **POST** /v1/ai/translate | Text translation (queued)


# **ai_chat**
> AiChatResponse ai_chat(ai_chat_request)

Chat completion via inference-core

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_chat_request import AiChatRequest
from model_plane_sdk.models.ai_chat_response import AiChatResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_chat_request = model_plane_sdk.AiChatRequest() # AiChatRequest | 

    try:
        # Chat completion via inference-core
        api_response = api_instance.ai_chat(ai_chat_request)
        print("The response of AiApi->ai_chat:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_chat: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_chat_request** | [**AiChatRequest**](AiChatRequest.md)|  | 

### Return type

[**AiChatResponse**](AiChatResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Chat response |  -  |
**400** | Invalid request |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **ai_documents**
> QueuedResponse ai_documents(ai_documents_request)

Document processing (queued)

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_documents_request import AiDocumentsRequest
from model_plane_sdk.models.queued_response import QueuedResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_documents_request = model_plane_sdk.AiDocumentsRequest() # AiDocumentsRequest | 

    try:
        # Document processing (queued)
        api_response = api_instance.ai_documents(ai_documents_request)
        print("The response of AiApi->ai_documents:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_documents: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_documents_request** | [**AiDocumentsRequest**](AiDocumentsRequest.md)|  | 

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Processing queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **ai_images**
> QueuedResponse ai_images(ai_images_request)

Image generation (queued)

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_images_request import AiImagesRequest
from model_plane_sdk.models.queued_response import QueuedResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_images_request = model_plane_sdk.AiImagesRequest() # AiImagesRequest | 

    try:
        # Image generation (queued)
        api_response = api_instance.ai_images(ai_images_request)
        print("The response of AiApi->ai_images:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_images: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_images_request** | [**AiImagesRequest**](AiImagesRequest.md)|  | 

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Generation queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **ai_realtime**
> QueuedResponse ai_realtime(ai_realtime_request)

Realtime session placeholder

Realtime sessions require WebSocket upgrade. Use /v1/invoke/stream for
SSE streaming instead.


### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_realtime_request import AiRealtimeRequest
from model_plane_sdk.models.queued_response import QueuedResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_realtime_request = model_plane_sdk.AiRealtimeRequest() # AiRealtimeRequest | 

    try:
        # Realtime session placeholder
        api_response = api_instance.ai_realtime(ai_realtime_request)
        print("The response of AiApi->ai_realtime:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_realtime: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_realtime_request** | [**AiRealtimeRequest**](AiRealtimeRequest.md)|  | 

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Not implemented — use SSE streaming |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **ai_speech**
> QueuedResponse ai_speech(ai_speech_request)

Text-to-speech synthesis (queued)

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_speech_request import AiSpeechRequest
from model_plane_sdk.models.queued_response import QueuedResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_speech_request = model_plane_sdk.AiSpeechRequest() # AiSpeechRequest | 

    try:
        # Text-to-speech synthesis (queued)
        api_response = api_instance.ai_speech(ai_speech_request)
        print("The response of AiApi->ai_speech:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_speech: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_speech_request** | [**AiSpeechRequest**](AiSpeechRequest.md)|  | 

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Synthesis queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **ai_translate**
> QueuedResponse ai_translate(ai_translate_request)

Text translation (queued)

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.ai_translate_request import AiTranslateRequest
from model_plane_sdk.models.queued_response import QueuedResponse
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
    api_instance = model_plane_sdk.AiApi(api_client)
    ai_translate_request = model_plane_sdk.AiTranslateRequest() # AiTranslateRequest | 

    try:
        # Text translation (queued)
        api_response = api_instance.ai_translate(ai_translate_request)
        print("The response of AiApi->ai_translate:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling AiApi->ai_translate: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **ai_translate_request** | [**AiTranslateRequest**](AiTranslateRequest.md)|  | 

### Return type

[**QueuedResponse**](QueuedResponse.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Translation queued |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

