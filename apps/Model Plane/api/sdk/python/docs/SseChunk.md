# SseChunk

A single chunk in the invoke/stream SSE response.

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**request_id** | **str** |  | [optional] 
**delta** | **str** |  | [optional] 
**done** | **bool** |  | [optional] 
**model_used** | **str** |  | [optional] 
**input_tokens** | **int** |  | [optional] 
**output_tokens** | **int** |  | [optional] 

## Example

```python
from model_plane_sdk.models.sse_chunk import SseChunk

# TODO update the JSON string below
json = "{}"
# create an instance of SseChunk from a JSON string
sse_chunk_instance = SseChunk.from_json(json)
# print the JSON string representation of the object
print(SseChunk.to_json())

# convert the object into a dict
sse_chunk_dict = sse_chunk_instance.to_dict()
# create an instance of SseChunk from a dict
sse_chunk_from_dict = SseChunk.from_dict(sse_chunk_dict)
```
[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


