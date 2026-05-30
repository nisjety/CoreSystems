# Capability


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**name** | **str** |  | [optional] 
**kind** | **str** |  | [optional] 
**version** | **str** |  | [optional] 
**description** | **str** |  | [optional] 
**risk_level** | **str** |  | [optional] 
**lazy_load** | **bool** |  | [optional] 
**scope** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.capability import Capability

# TODO update the JSON string below
json = "{}"
# create an instance of Capability from a JSON string
capability_instance = Capability.from_json(json)
# print the JSON string representation of the object
print(Capability.to_json())

# convert the object into a dict
capability_dict = capability_instance.to_dict()
# create an instance of Capability from a dict
capability_from_dict = Capability.from_dict(capability_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


