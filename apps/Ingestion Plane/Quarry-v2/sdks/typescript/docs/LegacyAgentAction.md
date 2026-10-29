
# LegacyAgentAction


## Properties

Name | Type
------------ | -------------
`type` | string
`url` | string
`selector` | string
`x` | number
`y` | number
`text` | string
`key` | string
`target` | string
`deltaX` | number
`deltaY` | number
`value` | string
`ms` | number
`timeoutMs` | number
`fullPage` | boolean
`script` | string

## Example

```typescript
import type { LegacyAgentAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "type": null,
  "url": null,
  "selector": null,
  "x": null,
  "y": null,
  "text": null,
  "key": null,
  "target": null,
  "deltaX": null,
  "deltaY": null,
  "value": null,
  "ms": null,
  "timeoutMs": null,
  "fullPage": null,
  "script": null,
} satisfies LegacyAgentAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as LegacyAgentAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


