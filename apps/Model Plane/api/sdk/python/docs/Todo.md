# Todo


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**thread_id** | **str** |  | [optional] 
**run_id** | **str** |  | [optional] 
**assignee** | **str** |  | [optional] 
**title** | **str** |  | [optional] 
**description** | **str** |  | [optional] 
**state** | [**TodoState**](TodoState.md) |  | [optional] 
**priority** | **str** |  | [optional] 
**blocked_by** | **List[str]** |  | [optional] 

## Example

```python
from model_plane_sdk.models.todo import Todo

# TODO update the JSON string below
json = "{}"
# create an instance of Todo from a JSON string
todo_instance = Todo.from_json(json)
# print the JSON string representation of the object
print(Todo.to_json())

# convert the object into a dict
todo_dict = todo_instance.to_dict()
# create an instance of Todo from a dict
todo_from_dict = Todo.from_dict(todo_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


