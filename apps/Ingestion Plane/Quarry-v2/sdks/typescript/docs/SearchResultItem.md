
# SearchResultItem


## Properties

Name | Type
------------ | -------------
`url` | string
`title` | string
`snippet` | string
`rank` | number
`provider` | string

## Example

```typescript
import type { SearchResultItem } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "title": null,
  "snippet": null,
  "rank": null,
  "provider": null,
} satisfies SearchResultItem

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as SearchResultItem
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


