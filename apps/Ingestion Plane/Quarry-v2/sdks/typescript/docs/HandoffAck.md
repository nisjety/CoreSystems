
# HandoffAck


## Properties

Name | Type
------------ | -------------
`jobId` | string
`acceptedAt` | Date
`rejected` | [Array&lt;HandoffAckRejectedInner&gt;](HandoffAckRejectedInner.md)
`effective` | [HandoffAckEffective](HandoffAckEffective.md)
`limits` | [HandoffAckLimits](HandoffAckLimits.md)

## Example

```typescript
import type { HandoffAck } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "jobId": null,
  "acceptedAt": null,
  "rejected": null,
  "effective": null,
  "limits": null,
} satisfies HandoffAck

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as HandoffAck
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


