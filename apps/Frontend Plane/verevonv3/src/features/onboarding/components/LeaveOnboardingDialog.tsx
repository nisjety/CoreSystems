import { Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'

type LeaveOnboardingDialogProps = {
  open: boolean
  onCancel: () => void
  onConfirm: () => void
}

/** "Back" from the first interactive onboarding step exits onboarding
 * entirely — sign out AND wipe the half-finished setup — so it asks first.
 * This is an in-app dialog rather than `window.confirm` on purpose: embedded
 * webviews and automation-driven browsers (the Claude desktop browser pane,
 * Playwright, Chrome's repeated-dialog suppression) auto-dismiss native
 * dialogs with `false` without ever rendering them, which made the Back
 * button look completely dead. Mirrors ContactSalesModal's structure. */
export function LeaveOnboardingDialog(props: LeaveOnboardingDialogProps) {
  const i18n = useI18n()

  return (
    <Show when={props.open}>
      <div
        role="presentation"
        onClick={() => props.onCancel()}
        onKeyDown={(event) => {
          if (event.key === 'Escape') props.onCancel()
        }}
        style={{
          position: 'fixed',
          inset: '0',
          background: 'rgba(17, 17, 17, 0.5)',
          display: 'flex',
          'align-items': 'center',
          'justify-content': 'center',
          'z-index': '1000',
        }}
      >
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="leave-onboarding-dialog-title"
          aria-describedby="leave-onboarding-dialog-body"
          // Move focus into the dialog when it mounts so Escape works and
          // screen readers announce it. `autofocus` only applies on page load,
          // not on dynamic insertion, so it is not enough here.
          tabindex={-1}
          ref={(el) => { requestAnimationFrame(() => el.focus()) }}
          onClick={(event) => event.stopPropagation()}
          style={{
            background: 'var(--surface, #fff)',
            'border-radius': '12px',
            padding: '28px',
            'max-width': '440px',
            width: '90%',
            'box-shadow': '0 20px 60px rgba(0, 0, 0, 0.25)',
          }}
        >
          <h2 id="leave-onboarding-dialog-title" style={{ margin: '0 0 12px', 'font-size': '1.25rem' }}>
            {i18n.tr('Avslutte oppsettet?', 'Leave setup?')}
          </h2>
          <p id="leave-onboarding-dialog-body" style={{ margin: '0 0 20px', 'line-height': '1.5' }}>
            {i18n.tr(
              'Du blir logget ut, og påbegynt oppsett blir slettet. Neste innlogging starter fra begynnelsen.',
              'You will be signed out and your unfinished setup will be deleted. Your next sign-in starts from the beginning.',
            )}
          </p>
          <div style={{ display: 'flex', gap: '12px', 'justify-content': 'flex-end' }}>
            <Button variant="secondary" onClick={() => props.onCancel()}>
              {i18n.tr('Avbryt', 'Cancel')}
            </Button>
            <Button variant="primary" onClick={() => props.onConfirm()}>
              {i18n.tr('Logg ut og slett oppsett', 'Sign out and delete setup')}
            </Button>
          </div>
        </div>
      </div>
    </Show>
  )
}
