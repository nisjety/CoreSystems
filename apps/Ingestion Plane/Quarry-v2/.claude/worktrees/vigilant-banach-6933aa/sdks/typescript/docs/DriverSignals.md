
# DriverSignals


## Properties

Name | Type
------------ | -------------
`actions` | Array&lt;string&gt;
`screenshot` | boolean
`pdf` | boolean
`priorBlockSignals` | number
`profileRequired` | boolean
`urlType` | string

## Example

```typescript
import type { DriverSignals } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "actions": null,
  "screenshot": null,
  "pdf": null,
  "priorBlockSignals": null,
  "profileRequired": null,
  "urlType": null,
} satisfies DriverSignals

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as DriverSignals
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


