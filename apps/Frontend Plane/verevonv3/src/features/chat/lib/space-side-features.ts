// The side features a Space thread does not get (DEC-V-R8). A review and a reasoning translation name their thread to the
// gateway, which answers them on that thread's connection. Model Gateway refuses a Space thread named for a side feature
// (409 scope_not_available_in_space), and the page names no thread on a Space thread. A Space thread's turns can be written
// under a customer gateway or a ChatGPT subscription, so with the BYO gateway switch on, the platform chain would take over
// that connection's work. The features are therefore not offered on a Space thread while the switch is on, as side chat is
// hidden there. With the switch off nothing changes: they run on the platform, as they always did.

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
