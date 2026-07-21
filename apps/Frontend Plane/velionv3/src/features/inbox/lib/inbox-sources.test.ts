import { describe, expect, it } from 'vitest'
import { deriveConnectedInboxSources } from './inbox-sources'

describe('deriveConnectedInboxSources', () => {
  it('keeps every active inbox-capable source visible before it has conversations', () => {
    const sources = deriveConnectedInboxSources([
      {
        id: 'conn-microsoft',
        providerKey: 'microsoft',
        displayName: 'Ima Fernandes Da Costa',
        status: 'active',
        capabilities: ['mail.read', 'teams.messages.read'],
        scopes: ['Mail.Read', 'ChannelMessage.Read.All'],
      },
      {
        id: 'conn-google',
        providerKey: 'google',
        displayName: 'Ima Dacosta',
        status: 'active',
        capabilities: ['gmail.read', 'gmail.send'],
        scopes: ['https://www.googleapis.com/auth/gmail.readonly'],
      },
      {
        id: 'conn-slack',
        providerKey: 'slack',
        displayName: 'integrationservice',
        status: 'active',
        capabilities: ['channels.history', 'chat.write'],
        scopes: ['channels:history'],
      },
      {
        id: 'conn-slack-secondary',
        providerKey: 'slack',
        displayName: 'Customer success',
        status: 'active',
        capabilities: ['channels.history'],
        scopes: ['channels:history'],
      },
      {
        id: 'conn-meta',
        providerKey: 'meta',
        displayName: 'Ima DaCosta',
        status: 'active',
        capabilities: [
          'social.inbox.read',
          'social.instagram.read',
          'social.messenger.manage',
          'social.whatsapp.manage',
        ],
        scopes: ['instagram_basic', 'pages_messaging', 'whatsapp_business_messaging'],
      },
      {
        id: 'conn-whatsapp-legacy',
        providerKey: 'whatsapp',
        displayName: 'Legacy WhatsApp',
        status: 'active',
        capabilities: ['social.inbox.read'],
        scopes: ['whatsapp_business_messaging'],
      },
      {
        id: 'conn-discord',
        providerKey: 'discord',
        displayName: 'Ima Da Costa',
        status: 'active',
        capabilities: ['messages.read'],
        scopes: ['bot'],
      },
      {
        id: 'conn-x',
        providerKey: 'x',
        displayName: 'Ima Da Costa',
        status: 'active',
        capabilities: ['social.inbox.read'],
        scopes: ['dm.read'],
      },
      {
        id: 'conn-slack-deleted',
        providerKey: 'slack',
        displayName: 'Old workspace',
        status: 'deleted',
        deletedAt: '2026-07-18T09:00:00Z',
        capabilities: ['channels.history'],
        scopes: ['channels:history'],
      },
    ])

    expect(sources.map(({ channel, label, href }) => ({ channel, label, href }))).toEqual([
      { channel: 'messenger', label: 'Messenger', href: '/inbox?view=mine&channel=messenger' },
      { channel: 'instagram', label: 'Instagram', href: '/inbox?view=mine&channel=instagram' },
      { channel: 'whatsapp', label: 'WhatsApp', href: '/inbox?view=mine&channel=whatsapp' },
      { channel: 'email', label: 'Outlook + Gmail', href: '/inbox?view=mine&channel=email' },
      { channel: 'slack', label: 'Slack', href: '/inbox?view=mine&channel=slack' },
      { channel: 'teams', label: 'Microsoft Teams', href: '/inbox?view=mine&channel=teams' },
      { channel: 'discord', label: 'Discord', href: '/inbox?view=mine&channel=discord' },
      { channel: 'x', label: 'Twitter / X', href: '/inbox?view=mine&channel=x' },
    ])
    expect(sources.find((source) => source.channel === 'slack')?.accountLabels).toEqual([
      'integrationservice',
      'Customer success',
    ])
    expect(sources.find((source) => source.channel === 'whatsapp')?.accountLabels).toEqual([
      'Ima DaCosta',
      'Legacy WhatsApp',
    ])
  })

  it('does not claim unified-provider channels that were not granted', () => {
    expect(deriveConnectedInboxSources([{
      id: 'conn-meta-profile-only',
      providerKey: 'meta',
      status: 'active',
      capabilities: ['social.profile.read'],
      scopes: ['public_profile'],
    }, {
      id: 'conn-whatsapp-profile-only',
      providerKey: 'whatsapp',
      status: 'active',
      capabilities: ['social.profile.read'],
      scopes: [],
    }])).toEqual([])
  })

  it('keeps legacy Facebook Page inboxes in Messenger instead of expanding them as unified Meta', () => {
    const sources = deriveConnectedInboxSources([{
      id: 'conn-facebook-page',
      providerKey: 'facebook-pages',
      status: 'active',
      capabilities: ['social.inbox.read'],
      scopes: ['pages_messaging'],
    }])

    expect(sources.map((source) => source.channel)).toEqual(['messenger'])
  })
})
