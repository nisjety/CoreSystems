
# CrawlScope


## Properties

Name | Type
------------ | -------------
`allowSubdomains` | boolean
`allowExternalLinks` | boolean
`crawlEntireDomain` | boolean
`includePaths` | Array&lt;string&gt;
`excludePaths` | Array&lt;string&gt;
`denyExtensions` | Array&lt;string&gt;
`legacy` | boolean

## Example

```typescript
import type { CrawlScope } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "allowSubdomains": null,
  "allowExternalLinks": null,
  "crawlEntireDomain": null,
  "includePaths": null,
  "excludePaths": null,
  "denyExtensions": null,
  "legacy": null,
} satisfies CrawlScope

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlScope
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


