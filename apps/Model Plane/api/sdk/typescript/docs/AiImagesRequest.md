
# AiImagesRequest


## Properties

Name | Type
------------ | -------------
`prompt` | string
`model` | string
`size` | string

## Example

```typescript
import type { AiImagesRequest } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "prompt": null,
  "model": null,
  "size": 1024x1024,
} satisfies AiImagesRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AiImagesRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


