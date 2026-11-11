
# BatchRequest


## Properties

Name | Type
------------ | -------------
`urls` | Array&lt;string&gt;
`redactPii` | string
`maxParallel` | number
`webhook` | [JobWebhook](JobWebhook.md)

## Example

```typescript
import type { BatchRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "urls": null,
  "redactPii": null,
  "maxParallel": null,
  "webhook": null,
} satisfies BatchRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as BatchRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


