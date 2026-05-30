# AiTranslateRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**text** | **str** |  | 
**source_language** | **str** |  | [optional] 
**target_language** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.ai_translate_request import AiTranslateRequest

# TODO update the JSON string below
json = "{}"
# create an instance of AiTranslateRequest from a JSON string
ai_translate_request_instance = AiTranslateRequest.from_json(json)
# print the JSON string representation of the object
print(AiTranslateRequest.to_json())

# convert the object into a dict
ai_translate_request_dict = ai_translate_request_instance.to_dict()
# create an instance of AiTranslateRequest from a dict
ai_translate_request_from_dict = AiTranslateRequest.from_dict(ai_translate_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


