# SubagentLineage


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**thread_id** | **str** |  | [optional] 
**max_depth** | **int** |  | [optional] 
**edges** | [**List[SubagentLineageEdgesInner]**](SubagentLineageEdgesInner.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.subagent_lineage import SubagentLineage

# TODO update the JSON string below
json = "{}"
# create an instance of SubagentLineage from a JSON string
subagent_lineage_instance = SubagentLineage.from_json(json)
# print the JSON string representation of the object
print(SubagentLineage.to_json())

# convert the object into a dict
subagent_lineage_dict = subagent_lineage_instance.to_dict()
# create an instance of SubagentLineage from a dict
subagent_lineage_from_dict = SubagentLineage.from_dict(subagent_lineage_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


