# QueuedResponse

Acknowledgement for async/queued AI modality requests.

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**status** | **str** |  | [optional] 
**note** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.queued_response import QueuedResponse

# TODO update the JSON string below
json = "{}"
# create an instance of QueuedResponse from a JSON string
queued_response_instance = QueuedResponse.from_json(json)
# print the JSON string representation of the object
print(QueuedResponse.to_json())

# convert the object into a dict
queued_response_dict = queued_response_instance.to_dict()
# create an instance of QueuedResponse from a dict
queued_response_from_dict = QueuedResponse.from_dict(queued_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


