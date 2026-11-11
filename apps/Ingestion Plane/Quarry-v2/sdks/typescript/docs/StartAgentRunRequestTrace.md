
# StartAgentRunRequestTrace

WS10.S4 trace context (Model Plane execution-core\'s run trace). Labels only, never authority. Every value must be 1 to 64 characters of letters, digits, underscore or hyphen; an invalid value is dropped, never a 400. `origin` is a hint: a user token yields `user` (or `agent` when the hint says so), a service token yields its hint or `api`. `run_id` and `agent` are kept only for a service token. The labels are stamped on the run\'s events and the origin on the browser-session projection (`runOrigin`, one of the five words: user, agent, schedule, webhook, api). See CONTRACTS.md, \"Agent run trace: browser events and run origin\".

## Properties

Name | Type
------------ | -------------
`runId` | string
`origin` | string
`agent` | [StartAgentRunRequestTraceAgent](StartAgentRunRequestTraceAgent.md)

## Example

```typescript
import type { StartAgentRunRequestTrace } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "runId": null,
  "origin": null,
  "agent": null,
} satisfies StartAgentRunRequestTrace

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as StartAgentRunRequestTrace
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


