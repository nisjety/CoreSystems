# ListCapabilities200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**capabilities** | [**List[Capability]**](Capability.md) |  | [optional] 
**has_more** | **bool** |  | [optional] 

## Example

```python
from model_plane_sdk.models.list_capabilities200_response import ListCapabilities200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ListCapabilities200Response from a JSON string
list_capabilities200_response_instance = ListCapabilities200Response.from_json(json)
# print the JSON string representation of the object
print(ListCapabilities200Response.to_json())

# convert the object into a dict
list_capabilities200_response_dict = list_capabilities200_response_instance.to_dict()
# create an instance of ListCapabilities200Response from a dict
list_capabilities200_response_from_dict = ListCapabilities200Response.from_dict(list_capabilities200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


