# Aquatiq marketing access checklist

Last checked: 2026-10-21. Aquatiq setup is paused at the user's request while
Studio development moves to triodelab.no and agenci.no. Preserve its existing
connections; do not replace or repurpose them for the new sites.

## Current verified state

- Search Console reporting works through Integration Core and Marketing Core
  into Pulse. The connected Google account is `ima.dacosta@aquatiq.com`.
  Its discovered permission is `siteFullUser`, not verified ownership.
- Analytics Admin and Analytics Data APIs are enabled in Google Cloud project
  `intergration-core`. The approved Google grant includes read-only Analytics
  and Search Console scopes, but returned no accessible GA4 properties.
- Public site `https://www.aquatiq.com/no` loads Google Tag Manager container
  `GTM-TXTKTMN` and Cookiebot. None of the three signed-in Google accounts could
  see a Tag Manager container. Container contents could not be inspected.
- Clarity project **Aquatiq**, `z1abticqmb`, is configured for `www.aquatiq.com`.
  Its approved export credential is securely connected through Integration
  Core. Microsoft returned zero sessions/users in the observed 72-hour window.
  No Clarity script was loaded in the inspected website state. This is not
  proof that no consent-blocked tag exists, nor proof of working collection.
- No tracking tag, consent setting, DNS record, or public website was changed.
- PageSpeed Insights is configured through Integration Core with an approved,
  restricted API key. Public-URL analysis does not require site ownership or
  Analytics account access.

## Minimum access needed

| System / purpose | Access or assistance needed | Current gap |
| --- | --- | --- |
| Search Console reports | Retain existing property access and approved read-only Integration Core grant | None for current reports; owner assistance only for ownership/property administration |
| PageSpeed Insights | Retain the existing restricted Integration Core API configuration; approved operator manages key/quota | No additional Aquatiq account access or OAuth grant needed for public-URL analysis |
| Existing GA4 reports | Property-level **Viewer** for the connected Google account, or an approved account already holding access | Property administrator must grant access to the correct existing property |
| Tracking through GTM | Container-level **Publish** on `GTM-TXTKTMN`, or an authorized publisher who applies the reviewed workspace changes | No signed-in account can access the container |
| Cookiebot consent | Existing authorized administrator/operator to inspect Statistics consent, tag triggers, and withdrawal behavior | Consent configuration has not been inspected or approved for Clarity |
| Clarity collection | Existing project administrator/operator to review installation, masking, consent and exclusions | Export works; website installation and visitor collection remain unverified |
| Website code/deployment | Repository and preview access; authorized deployment and rollback operator if code changes are required | No approved deployment path supplied; not necessary if GTM can implement everything safely |
| DNS / ownership | Owner assistance for a specific approved verification record, only if domain verification is required | Optional; not needed for the existing search reports |
| Integration Core operations | Existing approved operator for scoped provider configuration and protected secret mounts | Existing Google/Clarity reporting paths are configured; no replacement OAuth app or credentials needed |

Request property/container-scoped roles rather than account-wide Administrator.
GA4 Viewer is for reporting; configuration changes need a separately authorized
Editor/operator. GTM Publish permits publishing, not a reason to request account
Administrator. Permission grants and production publication still require explicit
approval at the action that changes access or collection.

### What the Aquatiq administrator should provide

1. Invite `ima.dacosta@aquatiq.com` to the correct existing GA4 property as Viewer.
   Supply its numeric property ID, web-stream ID, configured website origin,
   measurement ID, and property timezone. A `G-...` measurement ID is not a
   numeric property ID. Existing observed measurement IDs are discovery hints,
   not sufficient to choose a property or create a replacement.
2. In GTM, open **Admin → Container User Management** for `GTM-TXTKTMN`, and
   invite that Google account with Publish permission, or name an authorized
   publisher who will review and deploy the changes instead.
3. Identify the Cookiebot operator and website deployment owner. Confirm the
   approved public routes, purpose of analytics, privacy notice, masking and
   sensitive-route exclusions before enabling tracking.
4. Keep Clarity project `z1abticqmb` accessible to its authorized operator.
   The existing export token does not need to be recreated merely to install
   the public tracking project ID.

No one should send account passwords, OAuth tokens, API keys or export tokens
in chat, documents, source code or `.env` files. Use provider invitations and the
existing approved Integration Core secret-management path. Only the public
Clarity project ID belongs in a tracking tag; never its private export token.

## Acceptance checklist when access is available

- [ ] Discover the existing GA4 property/stream through Integration Core and
      confirm its origin before binding it to the Aquatiq website record.
- [ ] Inspect existing GTM tags, versions and consent rules; avoid duplicates.
- [ ] Review Cookiebot Statistics consent and the public privacy/cookie notice.
- [ ] Configure Clarity script loading only after analytics opt-in under the
      intended policy. Clarity's denied consent mode can still allow cookieless
      collection, so a denied flag alone does not establish no tracking.
- [ ] Keep advertising consent independent; do not silently grant advertising
      storage when analytics alone is accepted.
- [ ] Review masking, URLs/referrers and excluded routes. Do not add user IDs,
      form values or other personal identifiers as part of the MVP test.
- [ ] Preview fresh/no-choice, rejection, analytics-only acceptance, withdrawal,
      reload and navigation. Check no duplicate script and actual collection
      behavior, not only cookie presence/absence.
- [ ] Obtain explicit approval before publishing live visitor tracking.
- [ ] Publish through the authorized deployment path with a rollback version.
- [ ] Make a non-personal test visit without submitting forms; confirm activity
      in the correct Clarity project and GA4 Realtime/DebugView as appropriate.
- [ ] After provider processing, verify a fresh export/report reaches Pulse
      through Integration Core. Record source, site, scope and observation time.
      Clarity is project-wide, rolling UTC (up to 72 hours), not quarter history.
      Current successful exports are cached for six hours; respect the provider
      limit of ten requests per project per day. Do not fabricate traffic or
      repeatedly restart services to force reads.

## Optional future marketing access — not required for this slice

| Capability | Separate access to request only when implementing it |
| --- | --- |
| Google Ads reporting | Relevant ad-account read access and approved Google Ads API/developer-token setup through Integration Core |
| Meta Ads reporting | Relevant business/ad-account assets and approved reporting permissions through Integration Core |
| Social publishing | Explicit page/profile roles and app permissions for the intended channels |
| Campaign changes / spend | Separate approval, suitable management roles, budgets and audited actions; reporting access must not imply spending authority |
| DNS/property administration | Specific verification or administration approval; not blanket domain/registrar credentials |

## Moving development to Triodelab and Agenci

Use separate backend website records and per-site provider mappings. Do not
reuse Aquatiq's Clarity project, export credential, GA4 stream or Google account
grant as proof of access to another site. Code ownership does not automatically
grant provider reporting access.

The current local Marketing registry and managed bearer are fixed to the
Aquatiq test organization. The user subsequently chose to continue the local
no-auth Pulse demo instead of provisioning a separate workspace for this test.
Triodelab now has its own website record and separately approved Google grant
in that test scope; Aquatiq mappings were not repurposed. This is not production
tenant isolation. Before a real Triodelab deployment, establish its genuine
organization, scoped credentials and independent registry. Do not relabel the
Aquatiq tenant or copy its bindings into another tenant. The single-organization
MVP is not already a multi-tenant gateway.

## Provider references

- [Search Console roles](https://support.google.com/webmasters/answer/7687615?hl=en)
- [Analytics property access](https://support.google.com/analytics/answer/9305587?hl=en)
- [Tag Manager container permissions](https://support.google.com/tagmanager/answer/6107011?hl=en)
- [Clarity export authorization and limits](https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-data-export-api)
- [Clarity consent behavior](https://learn.microsoft.com/en-us/clarity/setup-and-installation/clarity-consent-api-v2)
