
# CrawlReport

WS14.S2. Present on a run_page response only when the request carried crawl.

## Properties

Name | Type
------------ | -------------
`v` | number
`scopeApplied` | boolean
`mode` | string
`linksSeen` | number
`linksAllowed` | number
`linksDenied` | number
`linksUnevaluated` | number
`denials` | [CrawlDenials](CrawlDenials.md)
`hostsAdded` | Array&lt;string&gt;
`crawlDelayMs` | number

## Example

```typescript
import type { CrawlReport } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "v": null,
  "scopeApplied": null,
  "mode": null,
  "linksSeen": null,
  "linksAllowed": null,
  "linksDenied": null,
  "linksUnevaluated": null,
  "denials": null,
  "hostsAdded": null,
  "crawlDelayMs": null,
} satisfies CrawlReport

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlReport
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


