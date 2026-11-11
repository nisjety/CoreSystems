
# LiveFrameData

One transient frame. The bytes are returned to the caller and are never stored.

## Properties

Name | Type
------------ | -------------
`mimeType` | string
`dataBase64` | string
`zdr` | boolean

## Example

```typescript
import type { LiveFrameData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "mimeType": null,
  "dataBase64": null,
  "zdr": null,
} satisfies LiveFrameData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as LiveFrameData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


