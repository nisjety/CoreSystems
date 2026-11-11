
# ParseData


## Properties

Name | Type
------------ | -------------
`markdown` | string
`kind` | string
`bytes` | number
`structure` | string
`truncated` | boolean
`pages` | number
`warnings` | [Array&lt;ParseWarning&gt;](ParseWarning.md)

## Example

```typescript
import type { ParseData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "markdown": null,
  "kind": null,
  "bytes": null,
  "structure": null,
  "truncated": null,
  "pages": null,
  "warnings": null,
} satisfies ParseData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ParseData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


