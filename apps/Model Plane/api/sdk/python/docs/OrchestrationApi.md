# model_plane_sdk.OrchestrationApi

All URIs are relative to *http://localhost:8080*

Method | HTTP request | Description
------------- | ------------- | -------------
[**approve_plan**](OrchestrationApi.md#approve_plan) | **POST** /v1/orchestration/plans/{plan_id}/approve | Approve a plan
[**cancel_run**](OrchestrationApi.md#cancel_run) | **POST** /v1/orchestration/runs/{run_id}/cancel | Request cancellation of a run
[**decide_approval**](OrchestrationApi.md#decide_approval) | **POST** /v1/orchestration/approvals/{approval_id}/decide | Approve or reject an approval request
[**get_approval**](OrchestrationApi.md#get_approval) | **GET** /v1/orchestration/approvals/{approval_id} | Get an approval by ID
[**get_plan**](OrchestrationApi.md#get_plan) | **GET** /v1/orchestration/plans/{plan_id} | Get a plan by ID
[**get_subagent_lineage**](OrchestrationApi.md#get_subagent_lineage) | **GET** /v1/orchestration/threads/{thread_id}/lineage | Get subagent lineage for a thread
[**get_todo**](OrchestrationApi.md#get_todo) | **GET** /v1/orchestration/todos/{todo_id} | Get a todo by ID
[**list_approvals**](OrchestrationApi.md#list_approvals) | **GET** /v1/orchestration/runs/{run_id}/approvals | List approvals for a run
[**list_plans**](OrchestrationApi.md#list_plans) | **GET** /v1/orchestration/runs/{run_id}/plans | List plans for a run
[**list_todos**](OrchestrationApi.md#list_todos) | **GET** /v1/orchestration/threads/{thread_id}/todos | List todos for a thread
[**reject_plan**](OrchestrationApi.md#reject_plan) | **POST** /v1/orchestration/plans/{plan_id}/reject | Reject a plan
[**resume_run**](OrchestrationApi.md#resume_run) | **POST** /v1/orchestration/runs/{run_id}/resume | Request resumption of a cancelled/paused run
[**update_todo_status**](OrchestrationApi.md#update_todo_status) | **POST** /v1/orchestration/todos/{todo_id}/status | Transition a todo to a new status


# **approve_plan**
> GetPlan200Response approve_plan(plan_id, transition_plan_body)

Approve a plan

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_plan200_response import GetPlan200Response
from model_plane_sdk.models.transition_plan_body import TransitionPlanBody
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    plan_id = 'plan_id_example' # str | 
    transition_plan_body = model_plane_sdk.TransitionPlanBody() # TransitionPlanBody | 

    try:
        # Approve a plan
        api_response = api_instance.approve_plan(plan_id, transition_plan_body)
        print("The response of OrchestrationApi->approve_plan:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->approve_plan: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **plan_id** | **str**|  | 
 **transition_plan_body** | [**TransitionPlanBody**](TransitionPlanBody.md)|  | 

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Plan approved |  -  |
**400** | Invalid request |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **cancel_run**
> CancelRun200Response cancel_run(run_id)

Request cancellation of a run

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.cancel_run200_response import CancelRun200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    run_id = 'run_id_example' # str | 

    try:
        # Request cancellation of a run
        api_response = api_instance.cancel_run(run_id)
        print("The response of OrchestrationApi->cancel_run:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->cancel_run: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **run_id** | **str**|  | 

### Return type

[**CancelRun200Response**](CancelRun200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Cancellation requested |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **decide_approval**
> GetApproval200Response decide_approval(approval_id, decide_approval_body)

Approve or reject an approval request

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.decide_approval_body import DecideApprovalBody
from model_plane_sdk.models.get_approval200_response import GetApproval200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    approval_id = 'approval_id_example' # str | 
    decide_approval_body = model_plane_sdk.DecideApprovalBody() # DecideApprovalBody | 

    try:
        # Approve or reject an approval request
        api_response = api_instance.decide_approval(approval_id, decide_approval_body)
        print("The response of OrchestrationApi->decide_approval:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->decide_approval: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **approval_id** | **str**|  | 
 **decide_approval_body** | [**DecideApprovalBody**](DecideApprovalBody.md)|  | 

### Return type

[**GetApproval200Response**](GetApproval200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Approval decided |  -  |
**400** | Invalid request |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_approval**
> GetApproval200Response get_approval(approval_id)

Get an approval by ID

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_approval200_response import GetApproval200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    approval_id = 'approval_id_example' # str | 

    try:
        # Get an approval by ID
        api_response = api_instance.get_approval(approval_id)
        print("The response of OrchestrationApi->get_approval:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->get_approval: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **approval_id** | **str**|  | 

### Return type

[**GetApproval200Response**](GetApproval200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Approval details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_plan**
> GetPlan200Response get_plan(plan_id)

Get a plan by ID

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_plan200_response import GetPlan200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    plan_id = 'plan_id_example' # str | 

    try:
        # Get a plan by ID
        api_response = api_instance.get_plan(plan_id)
        print("The response of OrchestrationApi->get_plan:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->get_plan: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **plan_id** | **str**|  | 

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Plan details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_subagent_lineage**
> GetSubagentLineage200Response get_subagent_lineage(thread_id)

Get subagent lineage for a thread

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_subagent_lineage200_response import GetSubagentLineage200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    thread_id = 'thread_id_example' # str | 

    try:
        # Get subagent lineage for a thread
        api_response = api_instance.get_subagent_lineage(thread_id)
        print("The response of OrchestrationApi->get_subagent_lineage:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->get_subagent_lineage: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **thread_id** | **str**|  | 

### Return type

[**GetSubagentLineage200Response**](GetSubagentLineage200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Lineage graph |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **get_todo**
> GetTodo200Response get_todo(todo_id)

Get a todo by ID

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_todo200_response import GetTodo200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    todo_id = 'todo_id_example' # str | 

    try:
        # Get a todo by ID
        api_response = api_instance.get_todo(todo_id)
        print("The response of OrchestrationApi->get_todo:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->get_todo: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **todo_id** | **str**|  | 

### Return type

[**GetTodo200Response**](GetTodo200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Todo details |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_approvals**
> ListApprovals200Response list_approvals(run_id, step_id=step_id)

List approvals for a run

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.list_approvals200_response import ListApprovals200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    run_id = 'run_id_example' # str | 
    step_id = 'step_id_example' # str |  (optional)

    try:
        # List approvals for a run
        api_response = api_instance.list_approvals(run_id, step_id=step_id)
        print("The response of OrchestrationApi->list_approvals:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->list_approvals: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **run_id** | **str**|  | 
 **step_id** | **str**|  | [optional] 

### Return type

[**ListApprovals200Response**](ListApprovals200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Approvals list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_plans**
> ListPlans200Response list_plans(run_id)

List plans for a run

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.list_plans200_response import ListPlans200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    run_id = 'run_id_example' # str | 

    try:
        # List plans for a run
        api_response = api_instance.list_plans(run_id)
        print("The response of OrchestrationApi->list_plans:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->list_plans: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **run_id** | **str**|  | 

### Return type

[**ListPlans200Response**](ListPlans200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Plans list |  -  |
**401** | Missing or invalid bearer token |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **list_todos**
> ListTodos200Response list_todos(thread_id, run_id=run_id)

List todos for a thread

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.list_todos200_response import ListTodos200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    thread_id = 'thread_id_example' # str | 
    run_id = 'run_id_example' # str |  (optional)

    try:
        # List todos for a thread
        api_response = api_instance.list_todos(thread_id, run_id=run_id)
        print("The response of OrchestrationApi->list_todos:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->list_todos: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **thread_id** | **str**|  | 
 **run_id** | **str**|  | [optional] 

### Return type

[**ListTodos200Response**](ListTodos200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Todos list |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **reject_plan**
> GetPlan200Response reject_plan(plan_id, transition_plan_body)

Reject a plan

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_plan200_response import GetPlan200Response
from model_plane_sdk.models.transition_plan_body import TransitionPlanBody
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    plan_id = 'plan_id_example' # str | 
    transition_plan_body = model_plane_sdk.TransitionPlanBody() # TransitionPlanBody | 

    try:
        # Reject a plan
        api_response = api_instance.reject_plan(plan_id, transition_plan_body)
        print("The response of OrchestrationApi->reject_plan:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->reject_plan: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **plan_id** | **str**|  | 
 **transition_plan_body** | [**TransitionPlanBody**](TransitionPlanBody.md)|  | 

### Return type

[**GetPlan200Response**](GetPlan200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Plan rejected |  -  |
**400** | Invalid request |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **resume_run**
> ResumeRun200Response resume_run(run_id)

Request resumption of a cancelled/paused run

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.resume_run200_response import ResumeRun200Response
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    run_id = 'run_id_example' # str | 

    try:
        # Request resumption of a cancelled/paused run
        api_response = api_instance.resume_run(run_id)
        print("The response of OrchestrationApi->resume_run:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->resume_run: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **run_id** | **str**|  | 

### Return type

[**ResumeRun200Response**](ResumeRun200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: Not defined
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Resumption requested |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

# **update_todo_status**
> GetTodo200Response update_todo_status(todo_id, transition_todo_body)

Transition a todo to a new status

### Example

* Bearer (JWT) Authentication (bearerAuth):

```python
import model_plane_sdk
from model_plane_sdk.models.get_todo200_response import GetTodo200Response
from model_plane_sdk.models.transition_todo_body import TransitionTodoBody
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
    api_instance = model_plane_sdk.OrchestrationApi(api_client)
    todo_id = 'todo_id_example' # str | 
    transition_todo_body = model_plane_sdk.TransitionTodoBody() # TransitionTodoBody | 

    try:
        # Transition a todo to a new status
        api_response = api_instance.update_todo_status(todo_id, transition_todo_body)
        print("The response of OrchestrationApi->update_todo_status:\n")
        pprint(api_response)
    except Exception as e:
        print("Exception when calling OrchestrationApi->update_todo_status: %s\n" % e)
```



### Parameters


Name | Type | Description  | Notes
------------- | ------------- | ------------- | -------------
 **todo_id** | **str**|  | 
 **transition_todo_body** | [**TransitionTodoBody**](TransitionTodoBody.md)|  | 

### Return type

[**GetTodo200Response**](GetTodo200Response.md)

### Authorization

[bearerAuth](../README.md#bearerAuth)

### HTTP request headers

 - **Content-Type**: application/json
 - **Accept**: application/json

### HTTP response details

| Status code | Description | Response headers |
|-------------|-------------|------------------|
**200** | Todo updated |  -  |
**400** | Invalid request |  -  |
**404** | Resource not found |  -  |

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

