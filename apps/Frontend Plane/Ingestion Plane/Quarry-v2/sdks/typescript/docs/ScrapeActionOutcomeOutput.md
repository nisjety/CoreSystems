
# ScrapeActionOutcomeOutput

A screenshot or pdf the action captured. Stored as an artifact (artifact_id) unless the run is under Zero Data Retention; under ZDR the bytes come inline (data_base64) within the shared inline cap. Neither field is set when the capture is over that cap.

## Properties

Name | Type
------------ | -------------
`kind` | string
`contentType` | string
`bytes` | number
`width` | number
`height` | number
`artifactId` | string
`dataBase64` | string

## Example

```typescript
import type { ScrapeActionOutcomeOutput } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "contentType": null,
  "bytes": null,
  "width": null,
  "height": null,
  "artifactId": null,
  "dataBase64": null,
} satisfies ScrapeActionOutcomeOutput

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeActionOutcomeOutput
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


