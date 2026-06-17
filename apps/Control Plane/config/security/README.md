# Control Plane Security Data

Local security data owned by Control Plane:

- `disposable-email-domains.txt` is enforced by `auth-core` signup.
- `nvd/` is the local NVD feed cache for vulnerability tooling.

Refresh commands:

```bash
python3 scripts/sync-disposable-email-domains.py
python3 scripts/sync-nvd-feeds.py
```

Keep provider tokens and keyed feed URLs in environment variables or a secret
manager, not in this directory.
