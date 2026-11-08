
# CrawlDenialSample


## Properties

Name | Type
------------ | -------------
`url` | string
`code` | string
`message` | string
`remedy` | string
`depth` | number

## Example

```typescript
import type { CrawlDenialSample } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "code": null,
  "message": null,
  "remedy": null,
  "depth": null,
} satisfies CrawlDenialSample

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlDenialSample
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


