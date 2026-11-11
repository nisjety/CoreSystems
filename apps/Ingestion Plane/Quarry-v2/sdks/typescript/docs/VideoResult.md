
# VideoResult

One video hit. Every field is present; a field the engine did not give is null.

## Properties

Name | Type
------------ | -------------
`url` | string
`title` | string
`thumbnailSrc` | string
`iframeSrc` | string
`author` | string
`length` | string
`publishedDate` | string
`content` | string

## Example

```typescript
import type { VideoResult } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "url": null,
  "title": null,
  "thumbnailSrc": null,
  "iframeSrc": null,
  "author": null,
  "length": null,
  "publishedDate": null,
  "content": null,
} satisfies VideoResult

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as VideoResult
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


