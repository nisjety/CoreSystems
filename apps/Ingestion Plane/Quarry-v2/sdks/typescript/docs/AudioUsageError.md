
# AudioUsageError


## Properties

Name | Type
------------ | -------------
`error` | string
`code` | string
`hint` | string
`nextActions` | Array&lt;string&gt;
`receivedUrl` | string

## Example

```typescript
import type { AudioUsageError } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "error": null,
  "code": null,
  "hint": null,
  "nextActions": null,
  "receivedUrl": null,
} satisfies AudioUsageError

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as AudioUsageError
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


