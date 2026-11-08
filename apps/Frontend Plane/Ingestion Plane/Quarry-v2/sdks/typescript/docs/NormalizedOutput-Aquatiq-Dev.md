
# NormalizedOutput


## Properties

Name | Type
------------ | -------------
`runId` | string
`url` | [UrlTriple](UrlTriple.md)
`status` | number
`fetchedAt` | Date
`fingerprint` | string
`formats` | [OutputFormats](OutputFormats.md)
`change` | [ChangeInfo](ChangeInfo.md)
`metadata` | [PageMetadata](PageMetadata.md)
`driver` | [DriverInfo](DriverInfo.md)

## Example

```typescript
import type { NormalizedOutput } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "url": null,
  "status": null,
  "fetchedAt": null,
  "fingerprint": null,
  "formats": null,
  "change": null,
  "metadata": null,
  "driver": null,
} satisfies NormalizedOutput

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as NormalizedOutput
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


