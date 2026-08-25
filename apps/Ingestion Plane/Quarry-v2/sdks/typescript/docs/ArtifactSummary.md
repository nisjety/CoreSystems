
# ArtifactSummary


## Properties

Name | Type
------------ | -------------
`artifactId` | string
`orgId` | string
`kind` | string
`bytes` | number
`sha256` | string
`createdAt` | Date
`sourceUrl` | string

## Example

```typescript
import type { ArtifactSummary } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "artifactId": null,
  "orgId": null,
  "kind": null,
  "bytes": null,
  "sha256": null,
  "createdAt": null,
  "sourceUrl": null,
} satisfies ArtifactSummary

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ArtifactSummary
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


