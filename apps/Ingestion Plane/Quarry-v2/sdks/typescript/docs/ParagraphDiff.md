
# ParagraphDiff


## Properties

Name | Type
------------ | -------------
`format` | string
`realDelta` | boolean
`added` | number
`removed` | number
`moved` | number
`unchanged` | number
`ops` | [Array&lt;TextDiffOp&gt;](TextDiffOp.md)
`truncated` | boolean

## Example

```typescript
import type { ParagraphDiff } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "format": null,
  "realDelta": null,
  "added": null,
  "removed": null,
  "moved": null,
  "unchanged": null,
  "ops": null,
  "truncated": null,
} satisfies ParagraphDiff

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ParagraphDiff
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


