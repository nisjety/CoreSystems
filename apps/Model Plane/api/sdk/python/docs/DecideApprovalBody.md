# DecideApprovalBody


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**decision** | **str** |  | 
**reason** | **str** |  | [optional] [default to '']

## Example

```python
from model_plane_sdk.models.decide_approval_body import DecideApprovalBody

# TODO update the JSON string below
json = "{}"
# create an instance of DecideApprovalBody from a JSON string
decide_approval_body_instance = DecideApprovalBody.from_json(json)
# print the JSON string representation of the object
print(DecideApprovalBody.to_json())

# convert the object into a dict
decide_approval_body_dict = decide_approval_body_instance.to_dict()
# create an instance of DecideApprovalBody from a dict
decide_approval_body_from_dict = DecideApprovalBody.from_dict(decide_approval_body_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


