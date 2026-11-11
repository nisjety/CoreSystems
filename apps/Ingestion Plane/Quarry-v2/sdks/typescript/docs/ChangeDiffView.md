
# ChangeDiffView

The data of GET /v1/change/diff (INNH-4)

## Properties

Name | Type
------------ | -------------
`sourceUrl` | string
`from` | [ChangeDiffSide](ChangeDiffSide.md)
`to` | [ChangeDiffSide](ChangeDiffSide.md)
`diffId` | string
`available` | boolean
`reason` | [ChangeDiffUnavailable](ChangeDiffUnavailable.md)
`diff` | [ParagraphDiff](ParagraphDiff.md)
`markdown` | [ChangeDiffMarkdown](ChangeDiffMarkdown.md)

## Example

```typescript
import type { ChangeDiffView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "sourceUrl": null,
  "from": null,
  "to": null,
  "diffId": null,
  "available": null,
  "reason": null,
  "diff": null,
  "markdown": null,
} satisfies ChangeDiffView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ChangeDiffView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


