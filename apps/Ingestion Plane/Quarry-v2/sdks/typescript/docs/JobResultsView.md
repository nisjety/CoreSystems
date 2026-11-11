
# JobResultsView

The data of GET /v1/{crawl,batch,jobs}/{job_id}/results (INNH-12)

## Properties

Name | Type
------------ | -------------
`kind` | string
`items` | [Array&lt;JobResult&gt;](JobResult.md)
`nextSeq` | number
`scanCapped` | boolean

## Example

```typescript
import type { JobResultsView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "items": null,
  "nextSeq": null,
  "scanCapped": null,
} satisfies JobResultsView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobResultsView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


