# AiChatResponseUsage


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**input_tokens** | **int** |  | [optional] 
**output_tokens** | **int** |  | [optional] 

## Example

```python
from model_plane_sdk.models.ai_chat_response_usage import AiChatResponseUsage

# TODO update the JSON string below
json = "{}"
# create an instance of AiChatResponseUsage from a JSON string
ai_chat_response_usage_instance = AiChatResponseUsage.from_json(json)
# print the JSON string representation of the object
print(AiChatResponseUsage.to_json())

# convert the object into a dict
ai_chat_response_usage_dict = ai_chat_response_usage_instance.to_dict()
# create an instance of AiChatResponseUsage from a dict
ai_chat_response_usage_from_dict = AiChatResponseUsage.from_dict(ai_chat_response_usage_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


