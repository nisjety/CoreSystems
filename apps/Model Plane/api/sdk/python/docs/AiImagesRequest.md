# AiImagesRequest


## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**prompt** | **str** |  | 
**model** | **str** |  | [optional] 
**size** | **str** |  | [optional] 

## Example

```python
from model_plane_sdk.models.ai_images_request import AiImagesRequest

# TODO update the JSON string below
json = "{}"
# create an instance of AiImagesRequest from a JSON string
ai_images_request_instance = AiImagesRequest.from_json(json)
# print the JSON string representation of the object
print(AiImagesRequest.to_json())

# convert the object into a dict
ai_images_request_dict = ai_images_request_instance.to_dict()
# create an instance of AiImagesRequest from a dict
ai_images_request_from_dict = AiImagesRequest.from_dict(ai_images_request_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


