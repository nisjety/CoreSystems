
# ProfileCookieView

One cookie with every attribute except its value.

## Properties

Name | Type
------------ | -------------
`name` | string
`domain` | string
`path` | string
`secure` | boolean
`httpOnly` | boolean
`session` | boolean
`expires` | Date
`sameSite` | string
`valueLen` | number
`value` | string

## Example

```typescript
import type { ProfileCookieView } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "name": null,
  "domain": null,
  "path": null,
  "secure": null,
  "httpOnly": null,
  "session": null,
  "expires": null,
  "sameSite": null,
  "valueLen": null,
  "value": null,
} satisfies ProfileCookieView

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ProfileCookieView
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


