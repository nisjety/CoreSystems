# PlanStep


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**title** | **str** |  | [optional] 
**operation** | **str** |  | [optional] 
**state** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.plan_step import PlanStep

# TODO update the JSON string below
json = "{}"
# create an instance of PlanStep from a JSON string
plan_step_instance = PlanStep.from_json(json)
# print the JSON string representation of the object
print(PlanStep.to_json())

# convert the object into a dict
plan_step_dict = plan_step_instance.to_dict()
# create an instance of PlanStep from a dict
plan_step_from_dict = PlanStep.from_dict(plan_step_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


