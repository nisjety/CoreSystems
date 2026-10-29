# TransitionTodoBody


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**status** | [**TodoState**](TodoState.md) |  | 
**reason** | **str** |  | [optional] [default to '']

## Example

```python
from model_plane_sdk.models.transition_todo_body import TransitionTodoBody

# TODO update the JSON string below
json = "{}"
# create an instance of TransitionTodoBody from a JSON string
transition_todo_body_instance = TransitionTodoBody.from_json(json)
# print the JSON string representation of the object
print(TransitionTodoBody.to_json())

# convert the object into a dict
transition_todo_body_dict = transition_todo_body_instance.to_dict()
# create an instance of TransitionTodoBody from a dict
transition_todo_body_from_dict = TransitionTodoBody.from_dict(transition_todo_body_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


