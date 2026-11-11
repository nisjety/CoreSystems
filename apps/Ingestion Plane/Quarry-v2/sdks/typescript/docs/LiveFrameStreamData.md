
# LiveFrameStreamData

The data of a frame event of the frame stream, whose SSE id is the sequence. The socket\'s frame message has the same fields with a type, and it has no agentCursor.

## Properties

Name | Type
------------ | -------------
`sequence` | number
`mimeType` | string
`dataBase64` | string
`zdr` | boolean
`agentCursor` | [LiveAgentCursorData](LiveAgentCursorData.md)

## Example

```typescript
import type { LiveFrameStreamData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "sequence": null,
  "mimeType": null,
  "dataBase64": null,
  "zdr": null,
  "agentCursor": null,
} satisfies LiveFrameStreamData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as LiveFrameStreamData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


