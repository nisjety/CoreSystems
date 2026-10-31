# Model Plane Protobuf

`baseline.binpb` is a buf image (`FileDescriptorSet`) representing the locked-in
public API of the Model Plane v2 protos. CI runs `buf breaking` against this
file to prevent backwards-incompatible changes.

To regenerate after an intentional, reviewed breaking change:

```bash
buf build "apps/Model Plane/proto" -o "apps/Model Plane/proto/baseline.binpb"
```

## Gates

CI (`.github/workflows/buf.yml`) runs two gates on every change under this folder:

- `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"`. The baseline is a real build of the committed protos (it was a 0-byte placeholder before 2026-10, which made the check pass against nothing). A job step also proves on every run that a deleted field turns the check red. Additive changes (new fields with new tag numbers, new messages, new RPCs) pass and do **not** require regenerating the baseline; refresh it only after an intentional, reviewed breaking change, built from the committed state (never from a dirty tree). `buf` is not installed on the Windows dev host; use the image:

  ```bash
  docker run --rm -v "$PWD:/w" -w /w bufbuild/buf build "apps/Model Plane/proto" -o "apps/Model Plane/proto/baseline.binpb"
  ```

- `buf lint` as a ratchet (`scripts/gates/buf-lint-ratchet.py`). The pre-existing findings (mostly missing comments) are listed in `buf-lint-baseline.json`, keyed by file, rule and message (not line number). Only new findings fail. Shrink the baseline as comments are added:

  ```bash
  docker run --rm -v "$PWD:/w" -w /w bufbuild/buf lint "apps/Model Plane/proto" --error-format=json > /tmp/lint.json
  python3 scripts/gates/buf-lint-ratchet.py --baseline "apps/Model Plane/proto/buf-lint-baseline.json" --update /tmp/lint.json
  ```

Proto field numbers and migration numbers are reserved in the build plan (`docs/plans/model-plane-chat-build-plan-2026-10-14.md`, sections 4.3 and 4.4): claim a number there before using it.
