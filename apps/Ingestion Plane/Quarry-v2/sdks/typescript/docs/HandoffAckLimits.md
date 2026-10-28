
# HandoffAckLimits

Set by /v1/crawl and /v1/batch. A crawl carries the page and depth bounds it will run with and why they differ from the request (`clamped_by` is `default` when the request was absent or zero, `hard` when it was above the ceiling, `entitlement` when the organization\'s remaining allowance was smaller, and empty when it was used as asked). A batch carries `urls`: how many URLs were asked for, accepted (the first ones, in order) and dropped. Counts only; never a URL. `remaining` is present only for a configured allowance (WS6.D-s).

## Properties

Name | Type
------------ | -------------
`maxPages` | [CrawlBound](CrawlBound.md)
`maxDepth` | [CrawlBound](CrawlBound.md)
`urls` | [BatchUrlsBound](BatchUrlsBound.md)

## Example

```typescript
import type { HandoffAckLimits } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "maxPages": null,
  "maxDepth": null,
  "urls": null,
} satisfies HandoffAckLimits

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as HandoffAckLimits
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


