
# AuditPageStructuredData


## Properties

Name | Type
------------ | -------------
`documents` | number
`invalid` | number
`types` | Array&lt;string&gt;
`partial` | boolean

## Example

```typescript
import type { AuditPageStructuredData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "documents": null,
  "invalid": null,
  "types": null,
  "partial": null,
} satisfies AuditPageStructuredData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AuditPageStructuredData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


