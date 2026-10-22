
# ApprovalCause

Why an approval was asked for. An allowlisted projection: the tool arguments, the call digest and the action key are never part of it.

## Properties

Name | Type
------------ | -------------
`code` | string
`tool` | string
`effectClass` | string
`riskCategory` | string
`ruleId` | string
`scopeOptions` | Array&lt;string&gt;
`stepUpRequired` | boolean

## Example

```typescript
import type { ApprovalCause } from '@model-plane/sdk'

// TODO: Update the object below with actual values
const example = {
  "code": null,
  "tool": null,
  "effectClass": null,
  "riskCategory": null,
  "ruleId": null,
  "scopeOptions": null,
  "stepUpRequired": null,
} satisfies ApprovalCause

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ApprovalCause
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


