import { trackingConfig } from "./consent-tracking";

/** Owner-created Triodelab web stream: public installation ID, not a credential. */
export const TRIODELAB_GA4_MEASUREMENT_ID = "G-ZEWPHF6X3K";

/** Public Clarity installation ID for Triodelab's own website project. */
export const TRIODELAB_CLARITY_PROJECT_ID = "z5gsvus553";

/** Preparing an ID must never activate analytics or bypass the shared consent gates. */
export function triodelabTrackingConfig(
  input: Parameters<typeof trackingConfig>[0] = {},
) {
  return trackingConfig({
    ...input,
    // Only an absent override uses the default. Empty/invalid overrides fail closed.
    measurementId: input.measurementId ?? TRIODELAB_GA4_MEASUREMENT_ID,
    clarityProjectId: input.clarityProjectId ?? TRIODELAB_CLARITY_PROJECT_ID,
  });
}
