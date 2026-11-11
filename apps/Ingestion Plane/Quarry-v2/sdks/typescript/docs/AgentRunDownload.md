
# AgentRunDownload

One admitted download of a run. No URL and no file name.

## Properties

Name | Type
------------ | -------------
`artifactId` | string
`mimeType` | string
`byteSize` | number
`sha256` | string
`admittedType` | string
`handoffState` | string

## Example

```typescript
import type { AgentRunDownload } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "artifactId": null,
  "mimeType": null,
  "byteSize": null,
  "sha256": null,
  "admittedType": null,
  "handoffState": null,
} satisfies AgentRunDownload

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AgentRunDownload
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


