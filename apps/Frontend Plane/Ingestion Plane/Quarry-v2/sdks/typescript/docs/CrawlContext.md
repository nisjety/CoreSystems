
# CrawlContext

WS14.S2. The crawl scope for one run_page call. Absent or null leaves the response unchanged. An unknown key, a bad value or an over-cap list is a 400 whose details.fields names each path. Hosts are bare names. The seed URL is used only when crawl_entire_domain is false.

## Properties

Name | Type
------------ | -------------
`v` | number
`kind` | string
`depth` | number
`maxDepth` | number
`seedHosts` | Array&lt;string&gt;
`seedUrl` | string
`scope` | [CrawlScope](CrawlScope.md)
`scopeMode` | string
`robots` | string

## Example

```typescript
import type { CrawlContext } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "v": null,
  "kind": null,
  "depth": null,
  "maxDepth": null,
  "seedHosts": null,
  "seedUrl": null,
  "scope": null,
  "scopeMode": null,
  "robots": null,
} satisfies CrawlContext

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CrawlContext
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


