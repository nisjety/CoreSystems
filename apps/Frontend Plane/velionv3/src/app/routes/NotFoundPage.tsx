import { ButtonLink } from '@/shared/ui/ButtonLink'

export default function NotFoundPage() {
  return (
    <section class="route-page route-page--centered">
      <p class="eyebrow">404</p>
      <h1>Surface not found</h1>
      <p class="route-page__lead">Velion v3 has a strict route map so product surfaces stay easy to audit.</p>
      <ButtonLink href="/dashboard" variant="primary" size="md">
        Back to dashboard
      </ButtonLink>
    </section>
  )
}
