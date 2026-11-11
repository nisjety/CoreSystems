
# ChangeDiffMarkdown

Only with include=markdown. Each text is cut at 1 MiB.

## Properties

Name | Type
------------ | -------------
`from` | string
`to` | string
`truncated` | boolean

## Example

```typescript
import type { ChangeDiffMarkdown } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "from": null,
  "to": null,
  "truncated": null,
} satisfies ChangeDiffMarkdown

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ChangeDiffMarkdown
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


