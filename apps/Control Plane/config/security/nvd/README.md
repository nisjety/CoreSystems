# NVD Feed Cache

This directory is the local Control Plane cache for NVD JSON 2.0 feeds.

Generate recent and modified CVE feeds with:

```bash
cd "apps/Control Plane"
python3 scripts/sync-nvd-feeds.py
```

Defaults:

- `NVD_FEED_CACHE_PATH=config/security/nvd`
- `NVD_FEED_URLS=` empty means the script mirrors NVD `recent` and `modified`
  CVE 2.0 feeds.

The generated JSON files and manifest are local security tooling data. Do not
store API keys here.
