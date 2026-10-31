
# CrawlRequest


## Properties

Name | Type
------------ | -------------
`url` | string
`maxPages` | number
`maxDepth` | number
`allowSubdomains` | boolean
`allowExternalLinks` | boolean
`includePaths` | Array&lt;string&gt;
`excludePaths` | Array&lt;string&gt;
`denyExtensions` | Array&lt;string&gt;
`crawlEntireDomain` | boolean
`sitemap` | string
`ignoreQueryParameters` | boolean
`scrapeOptions` | object
`ignoreRobotsTxt` | boolean

## Example

```typescript
import type { CrawlRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "maxPages": null,
  "maxDepth": null,
  "allowSubdomains": null,
  "allowExternalLinks": null,
  "includePaths": null,
  "excludePaths": null,
  "denyExtensions": null,
  "crawlEntireDomain": null,
  "sitemap": null,
  "ignoreQueryParameters": null,
  "scrapeOptions": null,
  "ignoreRobotsTxt": null,
} satisfies CrawlRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


