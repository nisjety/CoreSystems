/**
 * Whether this build offers "run the approved plan" (WS14 S2a).
 *
 * The execute turn is only worth offering where the Model Plane gateway can run
 * it: its own switch (`MODEL_GATEWAY_PLAN_EXECUTE_ENABLED`) is off by default, and
 * with it off every execute turn is refused, typed. The gateway does not tell the
 * SPA which way the switch is set, so the build carries the same decision:
 * `VITE_PLAN_EXECUTE_ENABLED=true` is flipped together with the gateway switch and
 * the other coupled flags. Anything else, including unset, is off, and with it off
 * an approval behaves exactly as it did before this feature (no armed turn, no
 * prefilled draft, no pill).
 *
 * This is a convenience for the person, never authority: the gateway still
 * decides, and its typed refusal (`plan_execute_disabled`) stays the backstop.
 * Read at call time so a test can pin it.
 */
export const planExecuteEnabled = (): boolean =>
  import.meta.env.VITE_PLAN_EXECUTE_ENABLED === 'true'
