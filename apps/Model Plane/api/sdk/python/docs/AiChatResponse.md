# AiChatResponse


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **str** |  | [optional] 
**content** | **str** |  | [optional] 
**model_used** | **str** |  | [optional] 
**usage** | [**AiChatResponseUsage**](AiChatResponseUsage.md) |  | [optional] 

## Example

```python
from model_plane_sdk.models.ai_chat_response import AiChatResponse

# TODO update the JSON string below
json = "{}"
# create an instance of AiChatResponse from a JSON string
ai_chat_response_instance = AiChatResponse.from_json(json)
# print the JSON string representation of the object
print(AiChatResponse.to_json())

# convert the object into a dict
ai_chat_response_dict = ai_chat_response_instance.to_dict()
# create an instance of AiChatResponse from a dict
ai_chat_response_from_dict = AiChatResponse.from_dict(ai_chat_response_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


