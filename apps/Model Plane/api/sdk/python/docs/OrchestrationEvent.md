# OrchestrationEvent

SSE event from /v1/runs/{run_id}/events

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**event_type** | **str** |  | [optional] 
**data** | **Dict[str, object]** |  | [optional] 

## Example

```python
from model_plane_sdk.models.orchestration_event import OrchestrationEvent

# TODO update the JSON string below
json = "{}"
# create an instance of OrchestrationEvent from a JSON string
orchestration_event_instance = OrchestrationEvent.from_json(json)
# print the JSON string representation of the object
print(OrchestrationEvent.to_json())

# convert the object into a dict
orchestration_event_dict = orchestration_event_instance.to_dict()
# create an instance of OrchestrationEvent from a dict
orchestration_event_from_dict = OrchestrationEvent.from_dict(orchestration_event_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


