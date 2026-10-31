# ApprovalCause

Why an approval was asked for. An allowlisted projection: the tool arguments, the call digest and the action key are never part of it.

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**code** | **str** | Closed cause code. A code this build does not know is reported as unclassified_gate. | [optional]
**tool** | **str** | Tool the pause is on. Null for a gate that is not a tool call. | [optional]
**effect_class** | **str** | Declared side-effect class of the call, when the gate read one. | [optional]
**risk_category** | **str** | Browser risk category the proposed action was held under. | [optional]
**rule_id** | **str** | Standing rule that raised the pause, when one did. | [optional]
**scope_options** | **List[str]** | Scopes the decider may grant. Only once is offered today. | [optional]
**step_up_required** | **bool** | True when granting needs a recent re-authentication. | [optional]

## Example

```python
from model_plane_sdk.models.approval_cause import ApprovalCause

# TODO update the JSON string below
json = "{}"
# create an instance of ApprovalCause from a JSON string
approval_cause_instance = ApprovalCause.from_json(json)
# print the JSON string representation of the object
print(ApprovalCause.to_json())

# convert the object into a dict
approval_cause_dict = approval_cause_instance.to_dict()
# create an instance of ApprovalCause from a dict
approval_cause_from_dict = ApprovalCause.from_dict(approval_cause_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


