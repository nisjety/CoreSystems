import { useNavigate, useParams } from '@solidjs/router'
import { createEffect, createSignal, Match, Show, Switch } from 'solid-js'
import { acceptOrganizationInvitation } from '@/shared/api/auth-client'
import { ApiError } from '@/shared/api/http'
import { switchActiveOrganization } from '@/shared/api/organization-client'
import { getSession, loadSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'

const INVITATION_ID = /^[A-Za-z0-9_-]{1,256}$/

export default function AcceptInvitationPage() {
  const i18n = useI18n()
  const navigate = useNavigate()
  const params = useParams<{ invitationId: string }>()
  const session = getSession()
  const [submitting, setSubmitting] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const invitationId = () => params.invitationId?.trim() ?? ''

  createEffect(() => {
    if (session.status !== 'unauthenticated') return
    const returnTo = `/accept-invitation/${encodeURIComponent(invitationId())}`
    navigate(`/login?returnTo=${encodeURIComponent(returnTo)}`, { replace: true })
  })

  const acceptInvitation = async () => {
    if (submitting()) return
    if (!INVITATION_ID.test(invitationId())) {
      setError(i18n.tr('Denne invitasjonslenken er ugyldig.', 'This invitation link is invalid.'))
      return
    }

    setSubmitting(true)
    setError(null)
    try {
      const accepted = await acceptOrganizationInvitation(invitationId())
      // Acceptance creates membership but does not refresh Better Auth's
      // session-data cookie. Select the accepted org explicitly, then bypass
      // the cookie cache before entering any tenant-scoped surface.
      await switchActiveOrganization(accepted.invitation.organizationId)
      await loadSession({ disableAuthCookieCache: true })
      navigate(session.onboardingStatus === 'COMPLETED' ? '/dashboard' : '/onboarding', {
        replace: true,
      })
    } catch (cause) {
      if (
        cause instanceof ApiError &&
        cause.code === 'INVITATION_NOT_FOUND'
      ) {
        setError(i18n.tr('Denne invitasjonen er utløpt, allerede akseptert, eller ikke lenger gyldig.', 'This invitation is expired, already accepted, or no longer valid.'))
      } else if (cause instanceof ApiError && cause.status === 403) {
        setError(i18n.tr('Denne invitasjonen tilhører en annen bekreftet e-postadresse.', 'This invitation belongs to a different verified email address.'))
      } else {
        setError(i18n.tr('Invitasjonen kunne ikke aksepteres. Prøv igjen.', 'The invitation could not be accepted. Please try again.'))
      }
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main class="auth-invitation" aria-labelledby="invitation-title">
      <section class="auth-invitation__card">
        <p class="auth-invitation__eyebrow">{i18n.tr('Verevon arbeidsområde', 'Verevon workspace')}</p>
        <h1 id="invitation-title">{i18n.tr('Organisasjonsinvitasjon', 'Organization invitation')}</h1>
        <Switch>
          <Match when={session.status === 'idle' || session.status === 'loading'}>
            <p role="status">{i18n.tr('Sjekker økten din …', 'Checking your session…')}</p>
          </Match>
          <Match when={session.status === 'authenticated'}>
            <p>
              {i18n.tr('Aksepter denne invitasjonen som ', 'Accept this invitation as ')}
              <strong>{session.user?.email}</strong>
              {i18n.tr('. Verevon vil bytte til den inviterte organisasjonen etter at Auth Core har bekreftet den.', '. Verevon will switch to the invited organization after Auth Core verifies it.')}
            </p>
            <Show when={error()}>
              <p role="alert">{error()}</p>
            </Show>
            <button
              type="button"
              disabled={submitting() || !INVITATION_ID.test(invitationId())}
              onClick={() => void acceptInvitation()}
            >
              {submitting() ? i18n.tr('Aksepterer …', 'Accepting…') : i18n.tr('Aksepter invitasjon', 'Accept invitation')}
            </button>
          </Match>
        </Switch>
      </section>
    </main>
  )
}
