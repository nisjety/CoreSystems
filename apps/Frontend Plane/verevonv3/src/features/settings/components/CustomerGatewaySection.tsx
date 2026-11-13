import { createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { useI18n } from '@/shared/i18n'
import { getOrganizationAISettings } from '@/shared/api/organization-client'
import {
  createCustomerGateway,
  deleteCustomerGateway,
  listCustomerGateways,
  type CustomerGatewayConnection,
} from '@/shared/api/model-connections-client'

/** The model ids a gateway lists, one per line or comma-separated: trimmed, non-empty and unique. */
function parseModelIds(raw: string): string[] {
  const ids = raw
    .split(/[\n,]/)
    .map((id) => id.trim())
    .filter((id) => id !== '')
  return ids.filter((id, index) => ids.indexOf(id) === index)
}

/**
 * The organisation's own OpenAI-compatible model gateway (W4-BYO-6). An owner or admin registers one and
 * removes one. Integration Core seals the key; this form clears its key field as soon as it is submitted.
 * Behind `VITE_BYO_GATEWAY`, and hidden for a zero-retention organisation.
 */
export function CustomerGatewaySection(props: { orgId?: string | null }) {
  const i18n = useI18n()
  const enabled = import.meta.env.VITE_BYO_GATEWAY === 'true'
  const orgId = () => props.orgId?.trim() || false
  const [settings] = createResource(orgId, async (id) => {
    try {
      return await getOrganizationAISettings(id)
    } catch {
      return undefined
    }
  })

  const [version, setVersion] = createSignal(0)
  const [connections] = createResource(
    () => (enabled ? `v${version()}` : false),
    async () => {
      try {
        return await listCustomerGateways()
      } catch {
        return [] as CustomerGatewayConnection[]
      }
    },
  )

  const [name, setName] = createSignal('')
  const [baseUrl, setBaseUrl] = createSignal('')
  const [apiKey, setApiKey] = createSignal('')
  const [modelIds, setModelIds] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal<string | null>(null)
  const [failed, setFailed] = createSignal(false)

  async function submit(event: Event) {
    event.preventDefault()
    // The key leaves the field at once, whatever happens next: it is sent once and never kept here.
    const key = apiKey()
    setApiKey('')
    const ids = parseModelIds(modelIds())
    if (!name().trim() || !baseUrl().trim() || !key || ids.length === 0) {
      setFailed(true)
      setMessage(i18n.tr(
        'Fyll ut navn, adresse, nøkkel og minst én modell.',
        'Fill in a name, an address, a key and at least one model.',
      ))
      return
    }
    setBusy(true)
    setMessage(null)
    try {
      await createCustomerGateway({ name: name(), baseUrl: baseUrl(), apiKey: key, modelIds: ids })
      setName('')
      setBaseUrl('')
      setModelIds('')
      setFailed(false)
      setMessage(i18n.tr('Gatewayen er registrert.', 'The gateway was registered.'))
      setVersion((current) => current + 1)
    } catch {
      setFailed(true)
      setMessage(i18n.tr('Gatewayen kunne ikke registreres.', 'The gateway could not be registered.'))
    } finally {
      setBusy(false)
    }
  }

  async function remove(connection: CustomerGatewayConnection) {
    setBusy(true)
    setMessage(null)
    try {
      await deleteCustomerGateway(connection.id)
      setFailed(false)
      setMessage(i18n.tr('Gatewayen er fjernet.', 'The gateway was removed.'))
      setVersion((current) => current + 1)
    } catch {
      setFailed(true)
      setMessage(i18n.tr('Gatewayen kunne ikke fjernes.', 'The gateway could not be removed.'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={enabled && settings()?.zdr !== true}>
      <section class="verevon-settings-customer-gateway" aria-labelledby="verevon-customer-gateway-title">
        <h3 id="verevon-customer-gateway-title">{i18n.tr('Egen modellgateway', 'Your own model gateway')}</h3>
        <p role="note">
          {i18n.tr(
            'Ikke null datalagring. Lavest personvernnivå.',
            'Not zero data retention; lowest privacy tier.',
          )}
        </p>
        <Show when={(connections() ?? []).length > 0}>
          <ul>
            <For each={connections() ?? []}>
              {(connection) => (
                <li>
                  <span>{connection.name}</span>
                  <span>{connection.baseUrl}</span>
                  <span>{connection.modelIds.join(', ')}</span>
                  <button type="button" disabled={busy()} onClick={() => void remove(connection)}>
                    {i18n.tr(`Fjern ${connection.name}`, `Remove ${connection.name}`)}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <form onSubmit={(event) => void submit(event)}>
          <label>
            {i18n.tr('Navn', 'Name')}
            <input type="text" maxlength={120} value={name()} onInput={(event) => setName(event.currentTarget.value)} />
          </label>
          <label>
            {i18n.tr('Adresse (https)', 'Base URL (https)')}
            <input type="url" value={baseUrl()} onInput={(event) => setBaseUrl(event.currentTarget.value)} />
          </label>
          <label>
            {i18n.tr('API-nøkkel', 'API key')}
            <input
              type="password"
              autocomplete="off"
              value={apiKey()}
              onInput={(event) => setApiKey(event.currentTarget.value)}
            />
          </label>
          <label>
            {i18n.tr('Modeller (én per linje)', 'Models (one per line)')}
            <textarea value={modelIds()} onInput={(event) => setModelIds(event.currentTarget.value)} />
          </label>
          <button type="submit" disabled={busy()}>
            {i18n.tr('Registrer gateway', 'Register gateway')}
          </button>
        </form>
        <Show when={message()}>
          {(text) => <p role={failed() ? 'alert' : 'status'}>{text()}</p>}
        </Show>
      </section>
    </Show>
  )
}
