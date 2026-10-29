# TransitionPlanBody


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**target_state** | [**PlanState**](PlanState.md) |  | 
**reason** | **str** |  | [optional] [default to '']

## Example

```python
from model_plane_sdk.models.transition_plan_body import TransitionPlanBody

# TODO update the JSON string below
json = "{}"
# create an instance of TransitionPlanBody from a JSON string
transition_plan_body_instance = TransitionPlanBody.from_json(json)
# print the JSON string representation of the object
print(TransitionPlanBody.to_json())

# convert the object into a dict
transition_plan_body_dict = transition_plan_body_instance.to_dict()
# create an instance of TransitionPlanBody from a dict
transition_plan_body_from_dict = TransitionPlanBody.from_dict(transition_plan_body_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


