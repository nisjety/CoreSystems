# GetTodo200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**todo** | [**Todo**](Todo.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.get_todo200_response import GetTodo200Response

# TODO update the JSON string below
json = "{}"
# create an instance of GetTodo200Response from a JSON string
get_todo200_response_instance = GetTodo200Response.from_json(json)
# print the JSON string representation of the object
print(GetTodo200Response.to_json())

# convert the object into a dict
get_todo200_response_dict = get_todo200_response_instance.to_dict()
# create an instance of GetTodo200Response from a dict
get_todo200_response_from_dict = GetTodo200Response.from_dict(get_todo200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


