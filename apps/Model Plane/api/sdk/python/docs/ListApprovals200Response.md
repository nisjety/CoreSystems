# ListApprovals200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**approvals** | [**List[Approval]**](Approval.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.list_approvals200_response import ListApprovals200Response

# TODO update the JSON string below
json = "{}"
# create an instance of ListApprovals200Response from a JSON string
list_approvals200_response_instance = ListApprovals200Response.from_json(json)
# print the JSON string representation of the object
print(ListApprovals200Response.to_json())

# convert the object into a dict
list_approvals200_response_dict = list_approvals200_response_instance.to_dict()
# create an instance of ListApprovals200Response from a dict
list_approvals200_response_from_dict = ListApprovals200Response.from_dict(list_approvals200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


