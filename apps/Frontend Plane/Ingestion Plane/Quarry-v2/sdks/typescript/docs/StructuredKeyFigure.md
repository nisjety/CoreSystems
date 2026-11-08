
# StructuredKeyFigure


## Properties

Name | Type
------------ | -------------
`label` | string
`value` | string
`unit` | string
`period` | string

## Example

```typescript
import type { StructuredKeyFigure } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "label": null,
  "value": null,
  "unit": null,
  "period": null,
} satisfies StructuredKeyFigure

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StructuredKeyFigure
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


