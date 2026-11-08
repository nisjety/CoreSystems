
# RobotsFetchDenial

error.details.denial of the 403 SECURITY_BLOCKED a page fetch gets when robots.txt refuses it (WS14.S4). The path never carries the query.

## Properties

Name | Type
------------ | -------------
`code` | string
`message` | string
`remedy` | string
`userAgent` | string
`path` | string
`host` | string

## Example

```typescript
import type { RobotsFetchDenial } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "code": null,
  "message": null,
  "remedy": null,
  "userAgent": null,
  "path": null,
  "host": null,
} satisfies RobotsFetchDenial

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as RobotsFetchDenial
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


