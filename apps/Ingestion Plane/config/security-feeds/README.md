# Quarry Security Feeds

This directory holds the local URL reputation snapshot loaded by `quarry-edge`.

Generate it with:

```bash
cd "apps/Ingestion Plane"
python3 scripts/sync-security-feeds.py --force
```

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
