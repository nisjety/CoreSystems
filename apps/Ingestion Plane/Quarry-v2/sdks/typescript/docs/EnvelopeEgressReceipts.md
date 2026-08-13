# EnvelopeEgressReceipts

Ordered, redacted egress-boundary decisions for a live browser run. Receipt
URLs contain origin and path only; credentials, query strings, headers,
bodies, resolved IPs, and proxy details are never exposed.

`data.receipts` is ordered by `sequence`. If a bounded receipt buffer cannot
serve a continuous sequence after the requested cursor, Quarry returns a
conflict rather than silently returning incomplete proof.
