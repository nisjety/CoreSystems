
# FillOtpRefAction

Dark: refused 403 unless QUARRY_EDGE__FILL_OTP is on (default off). Fills a one-time code, read from the user\'s approved mail connection, into one exact input of the latest snapshot. The code is never on the wire in either direction: the caller names only the target and the approval. The approval is a single-use fill_otp approval grant (child of the run grant), used once per live run. Before any code is read, the page must pass these checks: https only; the target is in the top frame (a child-frame target is refused), and frame origin equals top origin and grant origin; exactly one visible required non-password input, the only visible text field on the page; no datalist, visible select, radio, listbox or QR code. The code is inserted through trusted input. A stale or absent target answers 409 TARGET_REPAIR_REQUIRED, and an API-key run is refused (api_key_sensitive_action_refused). The code is never in a receipt, event or response (receipt outcome otp_filled). This variant has no value field: a value, code or text key is ignored and never kept or echoed.

## Properties

Name | Type
------------ | -------------
`type` | string
`snapshotId` | string
`generation` | number
`refId` | string
`approvalGrantId` | string

## Example

```typescript
import type { FillOtpRefAction } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "type": null,
  "snapshotId": null,
  "generation": null,
  "refId": @e1,
  "approvalGrantId": null,
} satisfies FillOtpRefAction

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as FillOtpRefAction
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


