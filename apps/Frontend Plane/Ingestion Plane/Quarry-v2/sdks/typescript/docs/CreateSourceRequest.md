
# CreateSourceRequest


## Properties

Name | Type
------------ | -------------
`name` | string
`url` | string
`kind` | string
`monitor` | boolean
`preset` | string
`config` | { [key: string]: any; }

## Example

```typescript
import type { CreateSourceRequest } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "name": null,
  "url": null,
  "kind": null,
  "monitor": null,
  "preset": null,
  "config": null,
} satisfies CreateSourceRequest

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as CreateSourceRequest
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


