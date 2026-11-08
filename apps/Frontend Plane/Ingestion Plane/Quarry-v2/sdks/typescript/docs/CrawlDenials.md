
# CrawlDenials


## Properties

Name | Type
------------ | -------------
`counts` | { [key: string]: number; }
`samples` | [Array&lt;CrawlDenialSample&gt;](CrawlDenialSample.md)
`robots` | [Array&lt;CrawlRobotsDenial&gt;](CrawlRobotsDenial.md)

## Example

```typescript
import type { CrawlDenials } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "counts": null,
  "samples": null,
  "robots": null,
} satisfies CrawlDenials

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlDenials
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


