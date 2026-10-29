# TaskInput


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**name** | **str** |  | [optional] 
**description** | **str** |  | [optional] 
**kind** | **str** |  | [optional] 
**schedule** | **str** |  | [optional] 
**payload** | **Dict[str, object]** |  | [optional] 

## Example

```python
from model_plane_sdk.models.task_input import TaskInput

# TODO update the JSON string below
json = "{}"
# create an instance of TaskInput from a JSON string
task_input_instance = TaskInput.from_json(json)
# print the JSON string representation of the object
print(TaskInput.to_json())

# convert the object into a dict
task_input_dict = task_input_instance.to_dict()
# create an instance of TaskInput from a dict
task_input_from_dict = TaskInput.from_dict(task_input_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


