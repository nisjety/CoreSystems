
# ScrapeRequest


## Properties

Name | Type
------------ | -------------
`url` | string
`prevFingerprint` | string
`cache` | [CachePolicy](CachePolicy.md)
`zdr` | boolean
`signals` | [DriverSignals](DriverSignals.md)
`ingest` | boolean
`orgId` | string

## Example

```typescript
import type { ScrapeRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": https://example.com/article,
  "prevFingerprint": null,
  "cache": null,
  "zdr": null,
  "signals": null,
  "ingest": null,
  "orgId": null,
} satisfies ScrapeRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ScrapeRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


