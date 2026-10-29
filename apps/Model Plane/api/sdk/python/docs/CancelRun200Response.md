# CancelRun200Response


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**run_id** | **str** |  | [optional] 
**status** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.cancel_run200_response import CancelRun200Response

# TODO update the JSON string below
json = "{}"
# create an instance of CancelRun200Response from a JSON string
cancel_run200_response_instance = CancelRun200Response.from_json(json)
# print the JSON string representation of the object
print(CancelRun200Response.to_json())

# convert the object into a dict
cancel_run200_response_dict = cancel_run200_response_instance.to_dict()
# create an instance of CancelRun200Response from a dict
cancel_run200_response_from_dict = CancelRun200Response.from_dict(cancel_run200_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


