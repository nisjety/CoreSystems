# SubagentLineageEdgesInner


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**parent_run_id** | **str** |  | [optional] 
**child_run_id** | **str** |  | [optional] 
**role** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.subagent_lineage_edges_inner import SubagentLineageEdgesInner

# TODO update the JSON string below
json = "{}"
# create an instance of SubagentLineageEdgesInner from a JSON string
subagent_lineage_edges_inner_instance = SubagentLineageEdgesInner.from_json(json)
# print the JSON string representation of the object
print(SubagentLineageEdgesInner.to_json())

# convert the object into a dict
subagent_lineage_edges_inner_dict = subagent_lineage_edges_inner_instance.to_dict()
# create an instance of SubagentLineageEdgesInner from a dict
subagent_lineage_edges_inner_from_dict = SubagentLineageEdgesInner.from_dict(subagent_lineage_edges_inner_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


