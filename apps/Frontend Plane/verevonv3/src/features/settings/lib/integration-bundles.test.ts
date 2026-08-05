import { describe, expect, it } from 'vitest'
import { connectBundlesForProvider, instagramInboxConnectionRequest } from './integration-bundles'

describe('connectBundlesForProvider', () => {
  it('starts Meta with the focused Messenger consent bundle', () => {
    expect(connectBundlesForProvider('meta')).toEqual(['messenger'])
  })

  it('starts LinkedIn with identity-only consent before reviewed products', () => {
    expect(connectBundlesForProvider('linkedin')).toEqual(['onboarding'])
  })

  it.each(['slack', 'discord', 'x'])('uses the focused inbox consent bundle for %s', (provider) => {
    expect(connectBundlesForProvider(provider)).toEqual(['inbox'])
  })

  it('keeps the established full consent bundle for providers without staged consent', () => {
    expect(connectBundlesForProvider('google')).toEqual(['full'])
  })

  it('uses the separately configured Instagram OAuth provider for Instagram inbox consent', () => {
    expect(instagramInboxConnectionRequest()).toEqual({ providerKey: 'instagram', bundles: ['inbox'] })
  })
})
