# Test Checklist

## Mailbox Behavior

- Send a brand-new message to `support@` and confirm Zammad creates one new ticket in `Support::Triage`.
- Reply to that message from the same external thread and confirm Zammad updates the existing ticket instead of creating a new one.
- Close the ticket, reply again, and confirm the group follow-up behavior matches your configured policy.
- Send a message with an attachment and confirm the attachment is stored on the ticket article.
- Send a message to `help@` and confirm it lands in the same queue as `support@` if aliasing is enabled.
- Send a message to `billing@` and confirm it lands in `Support::Billing`.

## Outbound Identity

- Reply from a ticket in `Support::Triage` and confirm the outbound email uses the support sender identity.
- Reply from a ticket in `Support::Billing` and confirm the outbound email uses the billing sender identity.
- Confirm the ticket article history contains the outbound reply.

## Ticket Fields

- Confirm the custom fields exist and are visible to agents in ticket edit mode.
- Update `ai_status`, `ai_confidence`, and `ai_review_required` manually and confirm the values persist.
- Search for tickets by `request_type` and `source_channel_key` to confirm the fields are indexed as expected after Elasticsearch sync.

## Trigger Safety

- Confirm tagging triggers add the expected mailbox tags only once.
- Confirm disabled webhook triggers do not fire.
- Enable webhook triggers in a staging environment only after the receiver validates signatures and payload shape.

## Operational Readiness

- Confirm SLAs calculate for `Support::Triage` and `Support::Billing`.
- Confirm escalated tickets appear in the expected overview.
- Confirm backups write to the `zammad-backup` volume.
- Confirm Zammad restarts cleanly after object migrations.
