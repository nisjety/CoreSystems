# Local browser attachment compatibility rollout

This Windows-only helper preserves the current `model-gateway` and `execution-core` runtime configuration and supplies both with one dedicated HMAC key for the legacy session-core browser attachment compatibility path. It does not upgrade session-core or run database migrations. It requires both compatible images to have already been built and tested, Docker Compose and Git Bash, and the Windows account that owns the snapshot.

From PowerShell, run the following after reviewing the candidate image tags defined in the script:

```powershell
python .\scripts\browser-attach-compat-rollout.py prepare
python .\scripts\browser-attach-compat-rollout.py check
python .\scripts\browser-attach-compat-rollout.py rollback-check
```

`prepare` records the current containers and immutable candidate image IDs and generates a fresh key. It stores the snapshot encrypted with Windows DPAPI for the current user in `%TEMP%\verevon-attach-compat-rollout.dpapi`; it refuses to overwrite an existing file. Set `VEREVON_ATTACH_SNAPSHOT_PATH` to a different absolute path if preparing another rollout, and use the same value for every subsequent command. Protect and retain the snapshot: losing it prevents recovery of the dedicated key and original configuration through this helper. Docker administrators can inspect container environment values, so DPAPI does not protect the key from Docker administrators.

`check` and `rollback-check` only render and compare configurations. They do not recreate services. After independent compatibility, retention, readiness and end-to-end verification, `apply` is the explicit mutating command. `rollback` restores the original pinned images and environment. Both recreate only the two named services, with dependencies untouched. Inspect service health after either operation. If a partial operation fails, investigate before using `rollback`; no database state is rolled back.

If ordinary Compose later recreates either service and removes the key or restores an original image, run:

```powershell
python .\scripts\browser-attach-compat-rollout.py reapply-check
python .\scripts\browser-attach-compat-rollout.py reapply
```

The first command is a dry run. `reapply` accepts only the saved original or candidate image IDs, with either the exact original environment or that environment plus the protected matching key. It refuses configuration drift in security, host settings, ports, mounts, networks and other checked fields; it then restores the pinned candidate images and key. Candidate tags must still resolve to the saved image IDs. Rebuilding a candidate requires a fresh protected snapshot and another complete validation. The helper uses existing Compose files and environment files, and aborts when their effective configuration no longer matches the saved runtime. It does not repair drift automatically.

This helper only manages local container configuration. A successful check or reapply does not prove legacy event acceptance, ownership enforcement, retention safety, a live browser connection, cursor visibility, or answer delivery; verify those separately. It is not an automatic startup hook or a production deployment mechanism.
