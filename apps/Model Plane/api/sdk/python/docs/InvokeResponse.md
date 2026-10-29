# InvokeResponse


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**request_id** | **str** |  | [optional] 
**content** | **str** |  | [optional] 
**model_used** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.invoke_response import InvokeResponse

# TODO update the JSON string below
json = "{}"
# create an instance of InvokeResponse from a JSON string
invoke_response_instance = InvokeResponse.from_json(json)
# print the JSON string representation of the object
print(InvokeResponse.to_json())

# convert the object into a dict
invoke_response_dict = invoke_response_instance.to_dict()
# create an instance of InvokeResponse from a dict
invoke_response_from_dict = InvokeResponse.from_dict(invoke_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


