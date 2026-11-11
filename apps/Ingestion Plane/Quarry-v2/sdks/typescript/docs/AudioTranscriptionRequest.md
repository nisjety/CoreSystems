
# AudioTranscriptionRequest

Audio to transcribe. The audio_url passes the web policy before anything leaves the edge, and the Model Plane fetches it.

## Properties

Name | Type
------------ | -------------
`audioUrl` | string
`mime` | string
`language` | string
`provider` | string
`zdr` | boolean

## Example

```typescript
import type { AudioTranscriptionRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "audioUrl": null,
  "mime": null,
  "language": null,
  "provider": null,
  "zdr": null,
} satisfies AudioTranscriptionRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AudioTranscriptionRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


