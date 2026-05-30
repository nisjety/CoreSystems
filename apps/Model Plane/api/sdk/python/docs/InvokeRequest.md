# InvokeRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**content** | **str** | User message content (max 100 KB) | 
**model** | **str** | Model identifier. Falls back to DEFAULT_MODEL env. | [optional] 
**session_key** | **str** | Optional session key for context continuity. | [optional] 
**thread_id** | **str** | Optional thread ID for conversation threading. | [optional] 
**structured_output_schema** | **str** | JSON Schema string for structured output. | [optional] 
**zdr** | **bool** | Zero data retention — if true, no data is persisted. | [optional] [default to False]
**max_cost_usd** | **float** | Maximum cost in USD for this request. | [optional] 
**max_tokens** | **int** | Maximum output tokens. | [optional] 

## Example

```python
from model_plane_sdk.models.invoke_request import InvokeRequest

# TODO update the JSON string below
json = "{}"
# create an instance of InvokeRequest from a JSON string
invoke_request_instance = InvokeRequest.from_json(json)
# print the JSON string representation of the object
print(InvokeRequest.to_json())

# convert the object into a dict
invoke_request_dict = invoke_request_instance.to_dict()
# create an instance of InvokeRequest from a dict
invoke_request_from_dict = InvokeRequest.from_dict(invoke_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


