# Quarry Security Feeds

This directory holds the local URL reputation snapshot loaded by `quarry-edge`.

Generate it with:

```bash
cd "apps/Ingestion Plane"
python3 scripts/sync-security-feeds.py --force
```

To refresh the snapshot on a schedule, start the `security-feed-sync` compose service from `apps/Ingestion Plane` with `docker compose --profile security-feeds up -d security-feed-sync`. It sits behind the `security-feeds` profile, so a plain `docker compose up` never starts it. The service runs the sync script in a loop and sleeps `SECURITY_FEED_INTERVAL_S` seconds between runs (default 900; a value that is not 1 to 9 digits also falls back to 900). The shell raises any smaller value to 600 seconds, the 10-minute minimum that the feed terms impose, and the script also refuses to refetch within 600 seconds of its last sync (the `.meta.json` sidecar). The script writes each file under a temporary name and then renames it into place, so quarry-edge never reads a partial snapshot. The edge mounts this directory read-only and picks up a new file only when `QUARRY_SECURITY_SNAPSHOT_WATCH_S` is set to a positive number of seconds (default `0`, off); with the watch off, the edge keeps the list it loaded at boot until it restarts.

Defaults:

- `URLHAUS_CSV_URL=https://urlhaus.abuse.ch/downloads/csv_recent/`
- `PHISHTANK_CSV_URL=` is empty by default because many deployments use a keyed feed URL.
- `QUARRY_SECURITY_SNAPSHOT_PATH=config/security-feeds/quarry-security.json`
- `SECURITY_FEED_MAX_HOSTS=250000`

The generated `quarry-security.json` uses quarry-security's native shape:

```json
{
  "allowlist": [],
  "blocklist": ["malicious.example"]
}
```

Do not put API keys in this directory. Use environment variables or a secret
manager for keyed feed URLs.
