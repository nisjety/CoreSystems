# Approval


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**run_id** | **str** |  | [optional] 
**step_id** | **str** |  | [optional] 
**kind** | **str** |  | [optional] 
**state** | **str** |  | [optional] 
**requested_of** | **str** |  | [optional] 
**decided_by** | **str** |  | [optional] 
**decision_reason** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.approval import Approval

# TODO update the JSON string below
json = "{}"
# create an instance of Approval from a JSON string
approval_instance = Approval.from_json(json)
# print the JSON string representation of the object
print(Approval.to_json())

# convert the object into a dict
approval_dict = approval_instance.to_dict()
# create an instance of Approval from a dict
approval_from_dict = Approval.from_dict(approval_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


