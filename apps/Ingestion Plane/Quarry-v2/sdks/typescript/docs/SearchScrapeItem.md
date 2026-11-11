
# SearchScrapeItem

One hydrated result. Status ok carries markdown, chars and truncated. Status failed carries a closed code, and http_status when the code is http_status. A failure never carries page text, host or provider text.

## Properties

Name | Type
------------ | -------------
`url` | string
`position` | number
`status` | string
`markdown` | string
`chars` | number
`truncated` | boolean
`code` | string
`httpStatus` | number

## Example

```typescript
import type { SearchScrapeItem } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "position": null,
  "status": null,
  "markdown": null,
  "chars": null,
  "truncated": null,
  "code": null,
  "httpStatus": null,
} satisfies SearchScrapeItem

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchScrapeItem
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


