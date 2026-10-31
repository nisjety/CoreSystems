# Control Plane namespace cutover

Naming approved on 2026-10-29: infrastructure and domain subjects use
`controlplane`; product-facing names use `verevon`.

The coordinated source change uses `controlplane.<entity>.<verb>` and the
`CONTROLPLANE` JetStream stream. Publishers, subscriber filters, PubAck checks,
provisioner definitions, broker ACLs and matching external consumer contracts
are updated together. Session commands use the existing product contract
`verevon.session.<id>.command`.

## Existing deployments

A stream name change is not an in-place broker rename. The provisioner refuses
normal startup against the still-active prior stream rather than silently
creating a second topology or dropping stored messages. The migration command
is packaged in the Audit Core image. It runs read-only preflight by default.

1. Pause publishers, drain every old durable to zero pending and ack-pending,
   then pause consumers. Keep all affected workloads paused for the cutover.
2. With the scoped `control-shared-provisioner` credentials supplied through
   environment variables, run `nats-namespace-migrate` without flags.
3. Apply with `nats-namespace-migrate --apply --publishers-paused
   --snapshot=/private/path/control-namespace-before.json`. The snapshot path
   must be new and writable; it is created with mode 0600 before changes.
4. The command changes the prior stream's capture subjects to an archive-only
   namespace, preserving messages, stream name and existing durable positions.
   It creates the canonical stream and consumers from the current provisioner.
5. Deploy the matching broker ACLs and all coordinated producer/consumer source
   changes before resuming workloads. Verify PubAcks, consumer lag, projection
   delivery and GDPR fan-out with the actual scoped service identities.

The command refuses pending work, an ambiguous pair of active streams, a missing
snapshot, or a missing publisher-pause assertion. It never deletes messages,
streams or existing consumers. If target provisioning fails, keep publishers
paused and rerun after correcting the failure; the archived history remains
available and the operation can resume. Retain the snapshot and archived stream
under the existing retention policy. Rollback must be coordinated with stopped
publishers and the saved topology; do not reset consumers or restore subjects
while the new stream is actively capturing overlapping subjects.

Legacy names remain only in the migration recognizer and test fixtures so an
existing installation can be upgraded safely. No production broker cutover has
been performed by the source/test work.

## Fresh deployments

The normal `nats-provisioner` creates the canonical topology without migration
flags when no prior stream exists. Runtime identities retain their narrow
publish/bind/consume/ACK permissions; migration uses the topology-owner identity.
