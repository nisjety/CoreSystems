
# ChallengeSignal

What an observed page says about access. Advisory: Quarry never tries to get past a challenge, and a signal does not stop a run by itself. Only strength=strong is a verdict that a bot wall stands in front of the page; strength=non_signal names an ordinary access state (a login form, a consent banner, a paywall, a rate limit, a bare status) so it is not mistaken for one. A strong signal gets one recovery per host and run, a wait or a reload of the same URL; if the host still challenges, block_reason becomes challenge_loop, recovery becomes exhausted and Quarry answers further navigation to that host and interaction with its page with 403 CHALLENGE_DETECTED (details: reason, host, recovery=exhausted, next=user_takeover_or_other_source; host is left out under Zero Data Retention). Screenshots, reads, waits, scrolling, answering a dialog the page already opened, back and forward, and navigation to another host stay allowed; raw script (evaluate) is refused with the interaction. 

## Properties

Name | Type
------------ | -------------
`kind` | string
`confidence` | number
`evidence` | Array&lt;string&gt;
`requiresEscalation` | boolean
`strength` | string
`blockReason` | string
`recovery` | string

## Example

```typescript
import type { ChallengeSignal } from '@quarry/client'

// TODO: Update the object below with actual values
const example = {
  "kind": null,
  "confidence": null,
  "evidence": null,
  "requiresEscalation": null,
  "strength": null,
  "blockReason": null,
  "recovery": null,
} satisfies ChallengeSignal

console.log(example)

// Convert the instance to a JSON string
const exampleJSON: string = JSON.stringify(example)
console.log(exampleJSON)

// Parse the JSON string back to an object
const exampleParsed = JSON.parse(exampleJSON) as ChallengeSignal
console.log(exampleParsed)
```

[[Back to top]](#) [[Back to API list]](../README.md#api-endpoints) [[Back to Model list]](../README.md#models) [[Back to README]](../README.md)


