// The side features a Space thread does not get (DEC-V-R8). A review and a reasoning translation name their thread to the
// gateway, which answers them on that thread's connection. Model Gateway refuses a Space thread named for a side feature
// (409 scope_not_available_in_space), so the page names every saved thread, a Space thread included, and the gateway
// decides. A Space thread's turns can be written under a customer gateway or a ChatGPT subscription, so with the BYO gateway
// switch on the features are not offered on a Space thread, as side chat is hidden there: the platform chain would take over
// that connection's work. With the switch off nothing changes: the features run on the platform, as they always did.

/** The BYO gateway switch as the page reads it: `VITE_BYO_GATEWAY`, the strict literal `'true'` and nothing else. */
export function byoGatewaySwitchOn(): boolean {
  return import.meta.env.VITE_BYO_GATEWAY === 'true'
}

/**
 * Whether a review or a reasoning translation is withheld on this thread: the thread is a Space thread and the BYO gateway
 * switch is on. A thread that is not a Space thread is never withheld, and nothing is withheld while the switch is off.
 */
export function sideFeaturesWithheldOnSpace(spaceThread: boolean | undefined): boolean {
  return spaceThread === true && byoGatewaySwitchOn()
}

/**
 * Whether the ChatGPT subscription route is withheld on this thread (DEC-V-R8): a Space thread while the BYO gateway switch
 * is on. A Space's turns written under a subscription are a connection that no side feature of that thread can be served on,
 * so a Space thread does not take the route while the switch is on: it is not offered there, and a turn that would name it is
 * refused at submit with the draft kept. Nothing is withheld while the switch is off, or outside a Space.
 */
export function subscriptionRouteWithheldOnSpace(spaceThread: boolean | undefined): boolean {
  return spaceThread === true && byoGatewaySwitchOn()
}

/**
 * The thread a side feature names to the gateway: the open thread, unless it is temporary or there is none. A Space thread
 * is named too. Model Gateway answers a named Space thread with a typed refusal (`scope_not_available_in_space`), so the
 * gateway decides it, and a Space thread's side feature is never silently served on the platform chain because the page
 * left the thread out.
 */
export function sideFeatureThreadOf(threadId: string | null | undefined, temporary: boolean): string | undefined {
  return threadId && !temporary ? threadId : undefined
}

/**
 * The typed refusals Model Gateway gives a side feature for the thread it names (DEC-V-R8): a Space thread, a thread holding
 * a zero-retention or waived run, a thread written under two connections, and a connection thread named without its model.
 * Each is refused before any model call, and the same thread is refused the same way again, so none is worth a retry.
 */
const THREAD_REFUSAL_CODES: ReadonlySet<string> = new Set([
  'scope_not_available_in_space',
  'side_chat_thread_retention_refused',
  'connection_ambiguous',
  'side_chat_model_required',
])

/** Whether a gateway error code is one of the typed thread refusals above. */
export function isThreadRefusalCode(code: string | null | undefined): boolean {
  return code != null && THREAD_REFUSAL_CODES.has(code)
}
