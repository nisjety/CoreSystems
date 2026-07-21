import { Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { buttonClasses } from '@/shared/ui/button-classes'

type ContactSalesModalProps = {
  open: boolean
  onClose: () => void
  orgName?: string
  employeeCount?: number
  websiteUrl?: string
}

/** Enterprise/"Custom" pricing depends on what the org needs beyond what
 * Velion supports natively, so it is never a self-serve checkout — this
 * modal explains that and hands the inquiry to sales via email, pre-filled
 * with whatever onboarding already learned about the org. */
export function ContactSalesModal(props: ContactSalesModalProps) {
  const i18n = useI18n()

  const mailtoHref = () => {
    const subject = i18n.tr(
      `Tilpasset plan${props.orgName ? ` – ${props.orgName}` : ''}`,
      `Custom plan${props.orgName ? ` – ${props.orgName}` : ''}`,
    )
    const bodyLines = [
      i18n.tr(
        'Hei, vi ønsker en tilpasset plan for organisasjonen vår.',
        "Hi, we'd like a custom plan for our organization.",
      ),
      '',
      i18n.tr('Organisasjon: ', 'Organization: ') + (props.orgName ?? ''),
      props.employeeCount
        ? i18n.tr('Antall ansatte: ', 'Employee count: ') + String(props.employeeCount)
        : undefined,
      props.websiteUrl ? i18n.tr('Nettsted: ', 'Website: ') + props.websiteUrl : undefined,
      '',
      i18n.tr(
        'Dette trenger vi utover det som støttes ut av boksen:',
        'Here is what we need beyond what is supported out of the box:',
      ),
    ].filter((line): line is string => line !== undefined)

    const params = new URLSearchParams({ subject, body: bodyLines.join('\n') })
    return `mailto:hei@velion.ai?${params.toString()}`
  }

  return (
    <Show when={props.open}>
      <div
        role="presentation"
        onClick={props.onClose}
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
          aria-labelledby="contact-sales-modal-title"
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
          <h2 id="contact-sales-modal-title" style={{ margin: '0 0 12px', 'font-size': '1.25rem' }}>
            {i18n.tr('La oss snakke om en tilpasset plan', "Let's talk about a custom plan")}
          </h2>
          <p style={{ margin: '0 0 12px', 'line-height': '1.5' }}>
            {i18n.tr(
              'Prisen for en tilpasset plan avhenger av behovene deres — hva Velion allerede støtter ut av boksen, og hva som må bygges som en tilpasset integrasjon for dere.',
              'Custom plan pricing depends on your needs — what Velion already supports natively out of the box, versus what would need to be built as a custom integration for you.',
            )}
          </p>
          <p style={{ margin: '0 0 20px', 'line-height': '1.5' }}>
            {i18n.tr(
              'Send oss en e-post med hva dere trenger, så kommer salg tilbake med et tilpasset tilbud.',
              "Email us what you need, and sales will get back to you with a tailored offer.",
            )}
          </p>
          <div style={{ display: 'flex', gap: '12px', 'justify-content': 'flex-end' }}>
            <Button variant="secondary" onClick={props.onClose}>
              {i18n.tr('Lukk', 'Close')}
            </Button>
            <a href={mailtoHref()} class={buttonClasses({ variant: 'primary' })}>
              {i18n.tr('Send e-post til salg', 'Email sales')}
            </a>
          </div>
        </div>
      </div>
    </Show>
  )
}
