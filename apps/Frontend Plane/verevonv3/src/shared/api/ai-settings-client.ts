import { requestJson } from './http'

/**
 * The reply tones a person can pick (EP-8-S3). This is the closed set user-core stores and the
 * gateway checks; `default` asks for nothing extra.
 */
export const TONE_PRESETS = ['default', 'professional', 'friendly', 'direct', 'warm', 'neutral'] as const
export type TonePreset = (typeof TONE_PRESETS)[number]

/** user-core's AI settings, as `GET /api/v1/settings/ai` returns them (`apps/Control Plane/user-core`). */
export type AISettings = {
  aiEnabled: boolean
  modelPreference: string
  dataCollectionEnabled: boolean
  personalizationEnabled: boolean
  tonePreset: TonePreset
  /** Whether this deployment shows the tone selector (user-core `TONE_PRESETS_VISIBLE`). */
  tonePresetsAvailable: boolean
}

export function getAISettings(): Promise<AISettings> {
  return requestJson<AISettings>('/api/v1/settings/ai')
}

/**
 * Saves the reply tone. user-core writes every AI field back on a PUT, so the other four fields are
 * read first and sent unchanged: a PUT of the tone alone would reset them.
 */
export async function updateTonePreset(tonePreset: TonePreset): Promise<AISettings> {
  const current = await getAISettings()
  return requestJson<AISettings>('/api/v1/settings/ai', {
    method: 'PUT',
    body: JSON.stringify({
      aiEnabled: current.aiEnabled,
      modelPreference: current.modelPreference,
      dataCollectionEnabled: current.dataCollectionEnabled,
      personalizationEnabled: current.personalizationEnabled,
      tonePreset,
    }),
  })
}
