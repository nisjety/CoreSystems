// Norwegian-first mapping from gateway/auth error `code`s to user-facing copy.
//
// The gateway (and the Better Auth service it proxies) return a structured
// `{ code, message }` pair on every error response, but `message` is always
// English ops/dev text — never meant for a Norwegian end user to read verbatim.
// Callers should translate by `code` via `translateApiError`, not display
// `err.message` directly. Unknown/unmapped codes fall back to a safe, generic
// Norwegian message instead of leaking the raw English string.
//
// Convention: NEW codes get added here as they're identified as user-facing.
// This is the seam — not a claim that every gateway code is covered yet.

type ErrorCopy = { no: string; en: string }

type ApiErrorLike = { code?: string | null } | null | undefined

const GENERIC_FALLBACK: ErrorCopy = {
  no: 'Noe gikk galt. Prøv igjen, eller kontakt support hvis problemet vedvarer.',
  en: 'Something went wrong. Try again, or contact support if the problem persists.',
}

const ERROR_COPY: Record<string, ErrorCopy> = {
  // Cross-cutting / structural
  validation_error: { no: 'Noen av feltene er ikke gyldige. Sjekk og prøv igjen.', en: 'Some fields are invalid. Check and try again.' },
  invalid_input: { no: 'Forespørselen inneholder ugyldige data.', en: 'The request contains invalid data.' },
  invalid_json: { no: 'Forespørselen kunne ikke leses.', en: 'The request could not be read.' },
  unauthenticated: { no: 'Du må logge inn på nytt.', en: 'You need to sign in again.' },
  forbidden: { no: 'Du har ikke tilgang til å gjøre dette.', en: 'You do not have permission to do this.' },
  not_implemented: { no: 'Denne funksjonen er ikke tilgjengelig ennå.', en: 'This feature is not available yet.' },
  confirmation_required: { no: 'Denne handlingen må bekreftes først.', en: 'This action needs to be confirmed first.' },
  create_failed: { no: 'Kunne ikke opprette dette. Prøv igjen.', en: 'Could not create this. Try again.' },
  upstream_error: { no: 'En bakenforliggende tjeneste svarte med en feil. Prøv igjen.', en: 'A backing service returned an error. Try again.' },
  upstream_unavailable: { no: 'Tjenesten er utilgjengelig akkurat nå. Prøv igjen om litt.', en: 'The service is unavailable right now. Try again shortly.' },
  upstream_shape_error: { no: 'Fikk et uventet svar fra tjenesten. Prøv igjen.', en: 'Got an unexpected response from the service. Try again.' },
  upload_requires_multipart: { no: 'Opplasting krever filvedlegg, ikke bare tekst.', en: 'Upload requires a file attachment, not just text.' },

  // Organization / membership (onboarding-adjacent)
  no_active_org: { no: 'Velg en organisasjon først.', en: 'Select an organization first.' },
  org_scope_required: { no: 'Denne handlingen krever en valgt organisasjon.', en: 'This action requires an organization to be selected.' },
  organization_required: { no: 'En organisasjon må være valgt for å fortsette.', en: 'An organization must be selected to continue.' },
  organization_access_denied: { no: 'Du har ikke tilgang til denne organisasjonen.', en: 'You do not have access to this organization.' },
  organization_conflict: { no: 'Organisasjonen ble endret av noen andre. Last inn på nytt og prøv igjen.', en: 'The organization was changed by someone else. Reload and try again.' },
  organization_lookup_failed: { no: 'Fant ikke organisasjonen. Prøv igjen.', en: 'Could not find the organization. Try again.' },
  organization_not_ready: { no: 'Organisasjonen er ikke klar ennå. Prøv igjen om litt.', en: 'The organization is not ready yet. Try again shortly.' },
  organization_owner_required: { no: 'Kun organisasjonens eier kan gjøre dette.', en: 'Only the organization owner can do this.' },
  organization_provisioning_failed: { no: 'Kunne ikke sette opp organisasjonen. Prøv igjen.', en: 'Could not provision the organization. Try again.' },
  organization_recovery_required: { no: 'Organisasjonen må gjenopprettes før du kan fortsette. Kontakt support.', en: 'The organization needs to be recovered before you can continue. Contact support.' },
  organization_activation_failed: { no: 'Kunne ikke aktivere organisasjonen. Prøv igjen.', en: 'Could not activate the organization. Try again.' },
  organization_membership_required: { no: 'Du må være medlem av denne organisasjonen.', en: 'You need to be a member of this organization.' },
  organization_membership_stale: { no: 'Medlemskapet ditt er ikke oppdatert. Last inn siden på nytt.', en: 'Your membership is out of date. Reload the page.' },
  organization_membership_unavailable: { no: 'Kunne ikke bekrefte medlemskapet ditt akkurat nå.', en: 'Could not verify your membership right now.' },
  organization_membership_authority_unavailable: { no: 'Kunne ikke bekrefte medlemskapet ditt akkurat nå.', en: 'Could not verify your membership right now.' },
  membership_lookup_failed: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  membership_lookup_invalid: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  membership_response_invalid: { no: 'Kunne ikke slå opp medlemskap. Prøv igjen.', en: 'Could not look up membership. Try again.' },
  billing_admin_required: { no: 'Kun en administrator kan gjøre dette.', en: 'Only an admin can do this.' },

  // Inbox / tickets / conversations
  thread_id_required: { no: 'Ingen samtale er valgt.', en: 'No conversation is selected.' },
  conversation_membership_required: { no: 'Du har ikke tilgang til denne samtalen.', en: 'You do not have access to this conversation.' },
  conversation_upstream_target_rejected: { no: 'Meldingen kunne ikke sendes til mottakeren.', en: 'The message could not be delivered to the recipient.' },
  // A channel send is not a normal retry: the provider may have received it
  // even though we did not get a final response. Preserve that operational
  // distinction so an agent cannot accidentally double-send to a customer.
  delivery_unknown: { no: 'Vi kan ikke bekrefte om svaret ble sendt. Ikke send på nytt automatisk; avstem først.', en: 'We cannot confirm whether the reply was sent. Do not retry automatically; reconcile first.' },
  send_failed: { no: 'Kundekanalen godtok ikke svaret. Det ble ikke sendt.', en: 'The customer channel did not accept the reply. It was not sent.' },
  delivery_unavailable: { no: 'Kundekanalen er ikke satt opp for utsending. Svaret ble ikke sendt eller lagret.', en: 'The customer channel is not configured for sending. The reply was not sent or stored.' },
  support_intake_unavailable: { no: 'Support-tjenesten er utilgjengelig akkurat nå.', en: 'The support service is unavailable right now.' },
  auth_callback_unavailable: { no: 'Tilkoblingen er utilgjengelig akkurat nå. Prøv igjen.', en: 'The connection is unavailable right now. Try again.' },
  auth_callback_response_invalid: { no: 'Fikk et ugyldig svar under tilkobling. Prøv igjen.', en: 'Got an invalid response while connecting. Try again.' },
  auth_callback_response_too_large: { no: 'Svaret under tilkobling var for stort. Prøv igjen.', en: 'The response while connecting was too large. Try again.' },
  auth_callback_target_rejected: { no: 'Tilkoblingen ble avvist.', en: 'The connection was rejected.' },
  notification_upstream_target_rejected: { no: 'Varselet kunne ikke leveres.', en: 'The notification could not be delivered.' },

  // Knowledge base / crawl / documents
  scrape_failed: { no: 'Kunne ikke hente innhold fra siden. Prøv igjen.', en: 'Could not fetch content from the page. Try again.' },
  extraction_failed: { no: 'Kunne ikke hente ut innhold fra dette dokumentet.', en: 'Could not extract content from this document.' },
  translation_failed: { no: 'Kunne ikke oversette innholdet.', en: 'Could not translate the content.' },
  sharepoint_register_failed: { no: 'Kunne ikke koble til SharePoint. Prøv igjen.', en: 'Could not connect to SharePoint. Try again.' },
  invalid_target_language: { no: 'Ugyldig målspråk valgt.', en: 'Invalid target language selected.' },
  studio_project_not_found: { no: 'Fant ikke prosjektet.', en: 'Project not found.' },
  fingerprint_unavailable: { no: 'Kunne ikke identifisere kilden akkurat nå.', en: 'Could not identify the source right now.' },

  // Integrations / delegated auth
  delegated_auth_unavailable: { no: 'Denne integrasjonen er ikke koblet til ennå.', en: 'This integration is not connected yet.' },
  integration_auth_unavailable: { no: 'Denne integrasjonen er ikke koblet til ennå.', en: 'This integration is not connected yet.' },
  imports_auth_unavailable: { no: 'Import-tilkoblingen er ikke satt opp ennå.', en: 'The import connection is not set up yet.' },
  cost_auth_unavailable: { no: 'Kunne ikke hente kostnadsdata akkurat nå.', en: 'Could not fetch cost data right now.' },
  // Model Plane's pre-flight budget guard (model-gateway `budget.rs`): the one
  // hard stop on spend that exists. It refuses BEFORE the run starts, so the
  // copy must not imply a partial answer was cut off.
  budget_exceeded: {
    no: 'Forbrukstaket er nådd, så agenten startet ikke. En administrator kan endre taket under Innstillinger › Forbrukstak.',
    en: 'The spend ceiling has been reached, so the agent did not start. An administrator can change the ceiling under Settings › Spend ceiling.',
  },
  budget_unavailable: {
    no: 'Forbruket kunne ikke kontrolleres, så agenten startet ikke. Prøv igjen om litt.',
    en: 'Spend could not be verified, so the agent did not start. Try again shortly.',
  },
  shipping_auth_unavailable: { no: 'Fraktintegrasjonen er ikke koblet til ennå.', en: 'The shipping integration is not connected yet.' },
  social_core_unavailable: { no: 'Sosiale medier-tjenesten er utilgjengelig akkurat nå.', en: 'The social service is unavailable right now.' },
  search_provider_unconfigured: { no: 'Søketjenesten er ikke satt opp ennå.', en: 'The search provider is not configured yet.' },
  video_search_unavailable: { no: 'Videosøk er utilgjengelig akkurat nå.', en: 'Video search is unavailable right now.' },
  api_key_create_failed: { no: 'Kunne ikke opprette API-nøkkel. Prøv igjen.', en: 'Could not create API key. Try again.' },
  invalid_brand_theme: { no: 'Ugyldig temavalg.', en: 'Invalid theme selection.' },
  invalid_profile_scope: { no: 'Ugyldig tilgangsomfang for denne profilen.', en: 'Invalid scope for this profile.' },
  invalid_profile_update: { no: 'Kunne ikke oppdatere profilen med disse verdiene.', en: 'Could not update the profile with these values.' },

  // Browser control (advanced/internal surfaces — low pilot exposure, still safe to map)
  browser_human_takeover_active: { no: 'En person har overtatt styringen av denne nettleserøkten.', en: 'Human takeover is active for this browser session.' },
  browser_session_not_found: { no: 'Fant ikke nettleserøkten.', en: 'Browser session not found.' },
  browser_session_failed: { no: 'Nettleserøkten feilet. Prøv igjen.', en: 'The browser session failed. Try again.' },
  browser_session_invalid: { no: 'Denne nettleserøkten er ugyldig.', en: 'This browser session is invalid.' },
  browser_run_conflict: { no: 'En annen kjøring pågår allerede for denne økten.', en: 'Another run is already active for this session.' },
  browser_run_invalid: { no: 'Ugyldig nettleserkjøring.', en: 'Invalid browser run.' },
  browser_owner_unavailable: { no: 'Fant ikke eieren av denne økten.', en: 'Could not find the owner of this session.' },
  browser_store_unavailable: { no: 'Nettleserlagring er utilgjengelig akkurat nå.', en: 'Browser storage is unavailable right now.' },
  browser_artifact_failed: { no: 'Kunne ikke hente nettleserdata. Prøv igjen.', en: 'Could not fetch browser artifact. Try again.' },
  browser_artifact_too_large: { no: 'Nettleserdataene var for store.', en: 'The browser artifact was too large.' },
  browser_frame_failed: { no: 'Kunne ikke hente skjermbilde. Prøv igjen.', en: 'Could not fetch the frame. Try again.' },
  browser_frame_invalid: { no: 'Ugyldig skjermbilde.', en: 'Invalid frame.' },
  browser_observation_missing: { no: 'Ingen observasjon tilgjengelig for denne økten.', en: 'No observation available for this session.' },
  browser_visual_observation_invalid: { no: 'Ugyldig visuell observasjon.', en: 'Invalid visual observation.' },
  browser_ws_invalid_upstream: { no: 'Mistet forbindelsen til nettleserøkten. Prøv igjen.', en: 'Lost connection to the browser session. Try again.' },
  invalid_browser_artifact: { no: 'Ugyldige nettleserdata.', en: 'Invalid browser artifact.' },
  invalid_browser_frame: { no: 'Ugyldig skjermbilde.', en: 'Invalid frame.' },
  invalid_browser_goal: { no: 'Ugyldig mål for nettleserøkten.', en: 'Invalid goal for the browser session.' },
  invalid_browser_profile: { no: 'Ugyldig nettleserprofil.', en: 'Invalid browser profile.' },
  invalid_browser_run: { no: 'Ugyldig nettleserkjøring.', en: 'Invalid browser run.' },
  invalid_browser_session: { no: 'Ugyldig nettleserøkt.', en: 'Invalid browser session.' },
  invalid_browser_tab: { no: 'Ugyldig fane.', en: 'Invalid tab.' },
  invalid_browser_control_mode: { no: 'Ugyldig styringsmodus for nettleseren.', en: 'Browser control mode must be agent_control or human_takeover.' },

  // Better Auth passthrough (proxied verbatim by the gateway; UPPER_SNAKE_CASE per Better Auth's own convention)
  EMAIL_NOT_VERIFIED: { no: 'Bekreft e-postadressen din for å fortsette.', en: 'Verify your email address to continue.' },
  INVALID_EMAIL_OR_PASSWORD: { no: 'Feil e-post eller passord.', en: 'Invalid email or password.' },
  INVALID_OTP: { no: 'Feil engangskode. Prøv igjen.', en: 'Invalid one-time code. Try again.' },
  INVITATION_NOT_FOUND: { no: 'Fant ikke invitasjonen. Be om en ny.', en: 'Invitation not found. Ask for a new one.' },
  plan_upgrade_required: { no: 'Oppgrader planen din for å fortsette.', en: 'Upgrade your plan to continue.' },
  entitlement_required: { no: 'Denne funksjonen er ikke inkludert i planen din.', en: 'This feature is not included in your plan.' },
}

/**
 * Translate a caught error into safe, locale-aware copy. Looks the error's
 * `code` up in {@link ERROR_COPY}; unknown/missing codes use `fallback` if the
 * caller supplied one, otherwise a generic Norwegian/English message — never
 * the raw `message` from the gateway, which is always English ops text.
 */
export function translateApiError(
  err: unknown,
  tr: (noText: string, enText: string) => string,
  fallback?: ErrorCopy,
): string {
  const code = typeof (err as ApiErrorLike)?.code === 'string' ? (err as { code: string }).code : null
  const copy = (code && ERROR_COPY[code]) || fallback || GENERIC_FALLBACK
  return tr(copy.no, copy.en)
}
