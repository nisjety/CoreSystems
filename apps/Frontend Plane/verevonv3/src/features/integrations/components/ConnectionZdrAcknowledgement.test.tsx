// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import type { ConnectionZdrAssessment } from '@/shared/api/connection-zdr-client'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'

// ZDR-Z-20-to-Z-21b: the acknowledgement dialog shows Integration Core's text exactly as served, sends back the
// digest and text version it was shown with the variant and where it was acknowledged, reloads the new text on a
// 409 zdr_assessment_changed, shows nothing when nothing needs acknowledging, and renders and calls nothing with
// VITE_ZDR_WAIVER_UI off. It stores nothing in the browser.

const client = vi.hoisted(() => ({
  getConnectionZdr: vi.fn(),
  acknowledgeConnectionZdr: vi.fn(),
}))

vi.mock('@/shared/api/connection-zdr-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/connection-zdr-client')>()),
  getConnectionZdr: client.getConnectionZdr,
  acknowledgeConnectionZdr: client.acknowledgeConnectionZdr,
}))

const { ConnectionZdrAcknowledgement } = await import('./ConnectionZdrAcknowledgement')

function assessment(overrides: Partial<ConnectionZdrAssessment> = {}, title = 'Served title v1'): ConnectionZdrAssessment {
  return {
    connectionId: 'conn-1',
    providerKey: 'openai-codex-subscription',
    assessed: true,
    capability: 'not_supported',
    basis: 'provider_terms',
    digest: 'digest-v1',
    requiresAcknowledgement: true,
    wordingVersion: 'wording-1',
    wordingStatus: 'draft',
    acknowledgement: null,
    text: {
      key: 'zdr.ack.first_use',
      variant: 'first_use',
      locale: 'en',
      title,
      body: 'Served body: this connection keeps data under the provider terms.',
      continueLabel: 'Served continue',
      cancelLabel: 'Served cancel',
      sha256: 'abc',
    },
    ...overrides,
  }
}

function open(props: { variant?: 'org_zdr_on' | 'first_use' | 'org_zdr_off'; via?: 'connect' | 'first_use' | 'settings' } = {}) {
  const onClose = vi.fn()
  const onAcknowledged = vi.fn()
  render(() => (
    <I18nProvider>
      <ConnectionZdrAcknowledgement
        connectionId="conn-1"
        variant={props.variant ?? 'first_use'}
        acknowledgedVia={props.via ?? 'first_use'}
        onClose={onClose}
        onAcknowledged={onAcknowledged}
      />
    </I18nProvider>
  ))
  return { onClose, onAcknowledged }
}

beforeEach(() => {
  window.localStorage.setItem(localeStorageKey, 'en')
  client.getConnectionZdr.mockReset()
  client.acknowledgeConnectionZdr.mockReset()
  vi.stubEnv('VITE_ZDR_WAIVER_UI', 'true')
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  window.localStorage.clear()
})

describe('ConnectionZdrAcknowledgement', () => {
  it('renders the served text exactly, read for the app locale and the variant', async () => {
    client.getConnectionZdr.mockResolvedValue(assessment())
    open()
    expect(await screen.findByRole('heading', { name: 'Served title v1' })).toBeTruthy()
    expect(screen.getByText('Served body: this connection keeps data under the provider terms.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Served continue' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Served cancel' })).toBeTruthy()
    expect(client.getConnectionZdr).toHaveBeenCalledWith('conn-1', 'en', 'first_use')
  })

  it('sends the digest and text version it showed, with the variant and where it was acknowledged', async () => {
    client.getConnectionZdr.mockResolvedValue(assessment())
    const result = { id: 'ack-1' }
    client.acknowledgeConnectionZdr.mockResolvedValue(result)
    const { onClose, onAcknowledged } = open({ variant: 'first_use', via: 'first_use' })

    fireEvent.click(await screen.findByRole('button', { name: 'Served continue' }))

    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(client.acknowledgeConnectionZdr).toHaveBeenCalledTimes(1)
    expect(client.acknowledgeConnectionZdr).toHaveBeenCalledWith('conn-1', {
      digest: 'digest-v1',
      textVersion: 'wording-1',
      locale: 'en',
      variant: 'first_use',
      acknowledgedVia: 'first_use',
    })
    expect(onAcknowledged).toHaveBeenCalledWith(result)
  })

  it('reloads and shows the new text on 409 zdr_assessment_changed, then sends the new pair', async () => {
    client.getConnectionZdr
      .mockResolvedValueOnce(assessment())
      .mockResolvedValueOnce(assessment({ digest: 'digest-v2', wordingVersion: 'wording-2' }, 'Served title v2'))
    client.acknowledgeConnectionZdr
      .mockRejectedValueOnce(new ApiError('changed', 409, 'zdr_assessment_changed'))
      .mockResolvedValueOnce({ id: 'ack-2' })
    const { onClose } = open({ variant: 'org_zdr_on', via: 'connect' })

    fireEvent.click(await screen.findByRole('button', { name: 'Served continue' }))
    await screen.findByRole('heading', { name: 'Served title v2' })
    expect(onClose).not.toHaveBeenCalled()
    expect(client.getConnectionZdr).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('alert')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Served continue' }))
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(client.acknowledgeConnectionZdr).toHaveBeenLastCalledWith('conn-1', {
      digest: 'digest-v2',
      textVersion: 'wording-2',
      locale: 'en',
      variant: 'org_zdr_on',
      acknowledgedVia: 'connect',
    })
  })

  it('shows nothing and closes when nothing needs acknowledging', async () => {
    client.getConnectionZdr.mockResolvedValue(assessment({ requiresAcknowledgement: false }))
    const { onClose } = open()
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(screen.queryByText('Served title v1')).toBeNull()
    expect(client.acknowledgeConnectionZdr).not.toHaveBeenCalled()
  })

  it('offers no continue on the read-only org_zdr_off review', async () => {
    client.getConnectionZdr.mockResolvedValue(assessment())
    open({ variant: 'org_zdr_off', via: 'settings' })
    await screen.findByRole('heading', { name: 'Served title v1' })
    expect(screen.queryByRole('button', { name: 'Served continue' })).toBeNull()
  })

  it('shows a retryable failure on a 503 and never the raw server message', async () => {
    client.getConnectionZdr.mockRejectedValueOnce(new ApiError('raw integration-core text', 503, 'unavailable'))
    open()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).not.toContain('raw integration-core text')
    client.getConnectionZdr.mockResolvedValueOnce(assessment())
    fireEvent.click(screen.getByRole('button', { name: /Prøv igjen|Try again|Retry/ }))
    await screen.findByRole('heading', { name: 'Served title v1' })
  })

  it('stores nothing in the browser', async () => {
    client.getConnectionZdr.mockResolvedValue(assessment())
    client.acknowledgeConnectionZdr.mockResolvedValue({ id: 'ack-1' })
    const { onClose } = open()
    fireEvent.click(await screen.findByRole('button', { name: 'Served continue' }))
    await waitFor(() => expect(onClose).toHaveBeenCalled())
    const keysOf = (storage: Storage) => Array.from({ length: storage.length }, (_, index) => storage.key(index))
    const stored = [...keysOf(window.localStorage), ...keysOf(window.sessionStorage)]
    expect(stored.filter((key) => key !== localeStorageKey)).toEqual([])
  })

  it.each([undefined, 'false', '1', 'TRUE'])('renders and calls nothing with the switch %j', async (value) => {
    vi.stubEnv('VITE_ZDR_WAIVER_UI', value as string)
    client.getConnectionZdr.mockResolvedValue(assessment())
    open()
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(client.getConnectionZdr).not.toHaveBeenCalled()
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
