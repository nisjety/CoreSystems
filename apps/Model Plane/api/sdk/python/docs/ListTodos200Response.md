# ListTodos200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**todos** | [**List[Todo]**](Todo.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.list_todos200_response import ListTodos200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ListTodos200Response from a JSON string
list_todos200_response_instance = ListTodos200Response.from_json(json)
# print the JSON string representation of the object
print(ListTodos200Response.to_json())

# convert the object into a dict
list_todos200_response_dict = list_todos200_response_instance.to_dict()
# create an instance of ListTodos200Response from a dict
list_todos200_response_from_dict = ListTodos200Response.from_dict(list_todos200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


