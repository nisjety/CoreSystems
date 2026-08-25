
# ElementFingerprint

Read-only observed target identity used for repair and proof. It is not an executable selector and must never be sent as a raw action target.

## Properties

Name | Type
------------ | -------------
`fingerprintId` | string
`tag` | string
`normalizedText` | string
`attributes` | Array&lt;Array&lt;string&gt;&gt;
`structuralPath` | string
`logicalId` | string

## Example

```typescript
import type { ElementFingerprint } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "fingerprintId": null,
  "tag": null,
  "normalizedText": null,
  "attributes": null,
  "structuralPath": null,
  "logicalId": null,
} satisfies ElementFingerprint

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ElementFingerprint
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


