
# HitTestData

E3.3-h. The answer for one point. `hit` says whether the browser found a node under it. `ref` is set only when that node, or one of its accessibility ancestors, is a target of the caller\'s last observation of the run; `snapshotId` and `generation` then name that snapshot. Without a `ref`, `reason` says why. No backend node id and no frame id are carried.

## Properties

Name | Type
------------ | -------------
`hit` | boolean
`ref` | string
`snapshotId` | string
`generation` | number
`role` | string
`name` | string
`reason` | string
`zdr` | boolean

## Example

```typescript
import type { HitTestData } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "hit": null,
  "ref": null,
  "snapshotId": null,
  "generation": null,
  "role": null,
  "name": null,
  "reason": null,
  "zdr": null,
} satisfies HitTestData

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as HitTestData
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


