import { describe, expect, it } from 'vitest'
import { deriveConnectedEmailAccounts, deriveConnectedInboxSources, isDiscordInboxChannelAwaitingSetup, isMetaInboxChannelAwaitingAssetProvision } from './inbox-sources'

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
        metadata: {
          webhook_account_ids: 'page-1,ig-1,waba-1',
          meta_page_ids: 'page-1',
          meta_instagram_account_ids: 'ig-1',
          meta_whatsapp_business_account_ids: 'waba-1',
        },
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
        id: 'conn-linkedin',
        providerKey: 'linkedin',
        displayName: 'Verevon LinkedIn',
        status: 'active',
        capabilities: ['social.inbox.read'],
        scopes: [],
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
      { channel: 'linkedin', label: 'LinkedIn', href: '/inbox?view=mine&channel=linkedin' },
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

  it('treats a failed Discord sync as setup-blocked even after OAuth created the source', () => {
    expect(isDiscordInboxChannelAwaitingSetup([{
      id: 'conn-discord-failed',
      providerKey: 'discord',
      status: 'active',
      capabilities: ['messages.read'],
      scopes: ['bot'],
      lastSyncStatus: 'failed',
    }])).toBe(true)

    expect(isDiscordInboxChannelAwaitingSetup([{
      id: 'conn-discord-healthy',
      providerKey: 'discord',
      status: 'active',
      capabilities: ['messages.read'],
      scopes: ['bot'],
      lastSyncStatus: 'synced',
    }])).toBe(false)
  })

  it('waits for provider-confirmed Meta webhook assets before calling a social inbox connected', () => {
    const consentedOnlyConnection = {
      id: 'conn-meta-consented-only',
      providerKey: 'meta',
      status: 'active',
      capabilities: ['social.messenger.read', 'social.messenger.manage'],
      scopes: ['pages_show_list', 'pages_read_engagement', 'pages_manage_metadata', 'pages_messaging'],
    }

    expect(deriveConnectedInboxSources([consentedOnlyConnection])).toEqual([])
    expect(isMetaInboxChannelAwaitingAssetProvision([consentedOnlyConnection], 'messenger')).toBe(true)
    expect(isMetaInboxChannelAwaitingAssetProvision([consentedOnlyConnection], 'instagram')).toBe(false)
  })

  it('requires the matching Meta asset type for each social channel', () => {
    const pageOnlyConnection = {
      id: 'conn-meta-page-only',
      providerKey: 'meta',
      status: 'active',
      metadata: { webhook_account_ids: 'page-1', meta_page_ids: 'page-1' },
      capabilities: ['social.messenger.manage', 'social.inbox.read'],
      scopes: ['pages_messaging', 'instagram_manage_messages'],
    }

    expect(deriveConnectedInboxSources([pageOnlyConnection]).map((source) => source.channel)).toEqual(['messenger'])
    expect(isMetaInboxChannelAwaitingAssetProvision([pageOnlyConnection], 'instagram')).toBe(true)
  })

  it('derives each permissioned Gmail or Outlook mailbox separately and preserves declared shared mailboxes', () => {
    expect(deriveConnectedEmailAccounts([
      {
        id: 'conn-outlook',
        providerKey: 'microsoft',
        status: 'active',
        capabilities: ['mail.read'],
        metadata: {
          mailbox_address: 'ima.dacosta@coresystem.com',
          shared_mailboxes: 'testbruker@coresystem.com',
        },
      },
      {
        id: 'conn-gmail',
        providerKey: 'google',
        status: 'active',
        capabilities: ['gmail.read'],
        metadata: { mailbox_address: 'imamzambi64@gmail.com' },
      },
      {
        id: 'conn-not-mail',
        providerKey: 'microsoft',
        userEmail: 'teams-only@coresystem.com',
        status: 'active',
        capabilities: ['teams.messages.read'],
      },
    ])).toEqual([
      {
        id: 'conn-outlook',
        providerKey: 'microsoft',
        label: 'ima.dacosta@coresystem.com',
        sharedMailboxes: ['testbruker@coresystem.com'],
        syncHealth: 'unknown',
      },
      {
        id: 'conn-gmail',
        providerKey: 'google',
        label: 'imamzambi64@gmail.com',
        sharedMailboxes: [],
        syncHealth: 'unknown',
      },
    ])
  })

  it('derives mailbox sync health without treating a completed fetch as customer delivery', () => {
    expect(deriveConnectedEmailAccounts([
      {
        id: 'conn-outlook-synced',
        providerKey: 'microsoft',
        status: 'active',
        capabilities: ['mail.read'],
        lastSyncStatus: 'synced',
        lastSyncAt: '2026-08-05T11:00:00.000Z',
      },
      {
        id: 'conn-gmail-refresh',
        providerKey: 'google',
        status: 'needs_refresh',
        capabilities: ['gmail.read'],
        lastSyncStatus: 'failed',
      },
      {
        id: 'conn-outlook-failed',
        providerKey: 'microsoft',
        status: 'active',
        capabilities: ['mail.read'],
        lastSyncStatus: 'failed',
      },
      {
        id: 'conn-gmail-running',
        providerKey: 'google',
        status: 'active',
        capabilities: ['gmail.read'],
        lastSyncStatus: 'running',
      },
    ]).map(({ id, syncHealth, lastSyncAt }) => ({ id, syncHealth, lastSyncAt }))).toEqual([
      { id: 'conn-outlook-synced', syncHealth: 'synced', lastSyncAt: '2026-08-05T11:00:00.000Z' },
      { id: 'conn-outlook-failed', syncHealth: 'attention', lastSyncAt: undefined },
      { id: 'conn-gmail-refresh', syncHealth: 'needs_reconnect', lastSyncAt: undefined },
      { id: 'conn-gmail-running', syncHealth: 'syncing', lastSyncAt: undefined },
    ])
  })

  it('uses the provider-confirmed mailbox email before a generic provider label', () => {
    const accounts = deriveConnectedEmailAccounts([{
      id: 'conn-gmail',
      providerKey: 'google',
      providerName: 'Google',
      providerEmail: 'support@example.com',
      status: 'active',
      capabilities: ['gmail.read'],
    }])

    expect(accounts[0]?.label).toBe('support@example.com')
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
