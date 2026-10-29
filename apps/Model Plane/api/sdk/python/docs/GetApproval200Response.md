# GetApproval200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**approval** | [**Approval**](Approval.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.get_approval200_response import GetApproval200Response

# TODO update the JSON string below
json = "{}"
# create an instance of GetApproval200Response from a JSON string
get_approval200_response_instance = GetApproval200Response.from_json(json)
# print the JSON string representation of the object
print(GetApproval200Response.to_json())

# convert the object into a dict
get_approval200_response_dict = get_approval200_response_instance.to_dict()
# create an instance of GetApproval200Response from a dict
get_approval200_response_from_dict = GetApproval200Response.from_dict(get_approval200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


