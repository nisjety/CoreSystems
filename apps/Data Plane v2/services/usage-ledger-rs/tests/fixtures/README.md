# Fixtures of the usage ledger

`usage-envelopes.v2.json` is the contract between this crate and its consumer, data-orchestrator (Go):
real signed envelopes, each built by `UsageBuilder` from a provider answer and signed by the real
`EventSigner` of the producer that would emit it, with the public key that verifies it and the fields a
consumer must read out of it. It is generated, never edited:

    UPDATE_USAGE_FIXTURE=1 cargo test -p usage-ledger-rs --test envelope_fixture

The run signs with throwaway keys made on the spot (the private halves are never written down) and writes
the file and `usage-envelopes.v2.json.sha256`. Copy both to
`services/data-orchestrator-go/testdata/` in the same change, then run `scripts/check-usage-contract.sh`,
which fails when the two copies or their pins differ. The Go tests read the copy through their real
verifier, consumer and forwarder (see that directory's README).

The pin is the sha256 of the file with CRLF normalised to LF. A missing fixture fails the test: a contract
that cannot be read must never read as one that passed.
