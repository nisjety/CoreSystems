# AiSpeechRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**input** | **str** |  | 
**voice** | **str** |  | [optional] 
**model** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.ai_speech_request import AiSpeechRequest

# TODO update the JSON string below
json = "{}"
# create an instance of AiSpeechRequest from a JSON string
ai_speech_request_instance = AiSpeechRequest.from_json(json)
# print the JSON string representation of the object
print(AiSpeechRequest.to_json())

# convert the object into a dict
ai_speech_request_dict = ai_speech_request_instance.to_dict()
# create an instance of AiSpeechRequest from a dict
ai_speech_request_from_dict = AiSpeechRequest.from_dict(ai_speech_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


