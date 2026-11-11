
# AudioSpeechRequest

Text to speak. The fields are forwarded to the Model Plane as sent; the edge checks only their types.

## Properties

Name | Type
------------ | -------------
`input` | string
`voice` | string
`format` | string
`speed` | number
`provider` | string
`zdr` | boolean

## Example

```typescript
import type { AudioSpeechRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "input": null,
  "voice": null,
  "format": null,
  "speed": null,
  "provider": null,
  "zdr": null,
} satisfies AudioSpeechRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AudioSpeechRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


