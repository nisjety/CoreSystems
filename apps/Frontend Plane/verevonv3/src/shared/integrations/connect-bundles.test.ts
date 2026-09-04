import { describe, expect, it } from 'vitest'
import { connectBundlesForSources, instagramInboxConnectionRequest } from './connect-bundles'

describe('connectBundlesForSources', () => {
  describe('microsoft', () => {
    // Regression: a Microsoft 365 connection made from onboarding (Teams,
    // Outlook, SharePoint, OneDrive selected) carried no mail scope, so Support
    // still showed "Koble til Outlook" and required a second connection.
    it('grants knowledge and inbox for the full Microsoft 365 card', () => {
      expect(connectBundlesForSources('microsoft', ['teams', 'outlook', 'sharepoint', 'onedrive']))
        .toEqual(['onboarding', 'inbox'])
    })

    it('connects the whole provider with the same set every surface needs', () => {
      // Support ("Koble til Outlook"), Inbox and Settings pass no sources.
      expect(connectBundlesForSources('microsoft')).toEqual(['onboarding', 'inbox'])
      expect(connectBundlesForSources('microsoft', [])).toEqual(['onboarding', 'inbox'])
    })

    it('keeps a documents-only pick at the content grant', () => {
      expect(connectBundlesForSources('microsoft', ['sharepoint', 'onedrive'])).toEqual(['onboarding'])
    })

    it('adds the inbox bundle for Teams messages alone', () => {
      expect(connectBundlesForSources('microsoft', ['teams'])).toEqual(['inbox'])
    })

    it('normalizes provider aliases', () => {
      expect(connectBundlesForSources('microsoft-graph', ['outlook'])).toEqual(['inbox'])
      expect(connectBundlesForSources('Microsoft365')).toEqual(['onboarding', 'inbox'])
    })
  })

  describe('google', () => {
    it('collapses to full when the card includes calendar', () => {
      expect(connectBundlesForSources('google', ['google_drive', 'documents', 'gmail', 'calendar'])).toEqual(['full'])
      expect(connectBundlesForSources('google')).toEqual(['full'])
    })

    it('requests the content grant for a Knowledge-only pick', () => {
      // Knowledge used to request `onboarding` (drive.metadata only), which
      // cannot read document content.
      expect(connectBundlesForSources('google', ['google_drive', 'documents'])).toEqual(['knowledge'])
    })

    it('requests only the inbox grant for Gmail', () => {
      expect(connectBundlesForSources('google', ['gmail'])).toEqual(['inbox'])
    })
  })

  describe('fixed product-review bundles', () => {
    it('starts Meta with the focused Messenger consent bundle regardless of sources', () => {
      expect(connectBundlesForSources('meta')).toEqual(['messenger'])
      expect(connectBundlesForSources('meta', ['pages', 'instagram_business', 'whatsapp', 'ads'])).toEqual(['messenger'])
    })

    it('starts LinkedIn with identity-only consent before reviewed products', () => {
      expect(connectBundlesForSources('linkedin')).toEqual(['onboarding'])
      expect(connectBundlesForSources('linkedin', ['company_pages', 'posts'])).toEqual(['onboarding'])
    })

    it.each(['slack', 'discord', 'x'])('uses the focused inbox consent bundle for %s', (provider) => {
      expect(connectBundlesForSources(provider)).toEqual(['inbox'])
    })

    it('uses the separately configured Instagram OAuth provider for Instagram inbox consent', () => {
      expect(instagramInboxConnectionRequest()).toEqual({ providerKey: 'instagram', bundles: ['inbox'] })
    })
  })

  describe('content providers', () => {
    it('requests the knowledge grant for Slack channels and Notion pages', () => {
      expect(connectBundlesForSources('slack', ['channels'])).toEqual(['knowledge'])
      expect(connectBundlesForSources('notion', ['pages', 'databases'])).toEqual(['knowledge'])
      expect(connectBundlesForSources('github', ['issues'])).toEqual(['knowledge'])
    })

    it('unions groups when a card spans several', () => {
      expect(connectBundlesForSources('slack', ['channels', 'messages'])).toEqual(['knowledge', 'inbox'])
    })

    it('adds the identity preview bundle for sources no group claims', () => {
      expect(connectBundlesForSources('notion', ['comments'])).toEqual(['onboarding'])
      expect(connectBundlesForSources('slack', ['messages', 'emoji'])).toEqual(['inbox', 'onboarding'])
    })
  })

  describe('providers without a source policy', () => {
    it('keeps the established full grant for a whole-provider connect', () => {
      expect(connectBundlesForSources('tiktok')).toEqual(['full'])
      expect(connectBundlesForSources('snapchat')).toEqual(['full'])
    })

    it('keeps the safe preview bundle when sources were picked explicitly', () => {
      expect(connectBundlesForSources('tiktok', ['creator_profile', 'videos'])).toEqual(['onboarding'])
    })
  })
})
