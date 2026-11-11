
# AnswerErrorBody


## Properties

Name | Type
------------ | -------------
`error` | string
`code` | string
`hint` | string
`nextActions` | Array&lt;string&gt;
`reason` | string

## Example

```typescript
import type { AnswerErrorBody } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "error": null,
  "code": null,
  "hint": null,
  "nextActions": null,
  "reason": null,
} satisfies AnswerErrorBody

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AnswerErrorBody
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


