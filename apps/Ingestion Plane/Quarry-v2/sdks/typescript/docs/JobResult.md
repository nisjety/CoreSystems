
# JobResult

One page a job fetched. Fields after `fetched_at` are absent when the fetch event predates them or nothing was stored.

## Properties

Name | Type
------------ | -------------
`seq` | number
`url` | string
`status` | number
`contentType` | string
`title` | string
`fetchedAt` | Date
`wordCount` | number
`driver` | string
`markdownArtifactId` | string
`sourceArtifactId` | string
`bytes` | number

## Example

```typescript
import type { JobResult } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "seq": null,
  "url": null,
  "status": null,
  "contentType": null,
  "title": null,
  "fetchedAt": null,
  "wordCount": null,
  "driver": null,
  "markdownArtifactId": null,
  "sourceArtifactId": null,
  "bytes": null,
} satisfies JobResult

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as JobResult
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


