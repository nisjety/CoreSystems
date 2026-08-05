# API Contracts

Verevon v2 route handlers are the frontend L5 ingress. They normalize upstream plane responses into a small REST contract.

## Response Envelope

Successful resource response:

```json
{ "data": { "id": "example" } }
```

Cursor collection response:

```json
{
  "data": [],
  "meta": {
    "limit": 25,
    "hasNext": true,
    "nextCursor": "25"
  },
  "links": {
    "self": "/api/v1/conversations?limit=25",
    "next": "/api/v1/conversations?limit=25&cursor=25"
  }
}
```

Error response:

```json
{
  "error": {
    "code": "validation_error",
    "message": "Request validation failed",
    "details": [
      { "field": "limit", "code": "out_of_range", "message": "Use a value from 1 to 100." }
    ]
  }
}
```

## Routes

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/v1/health` | App health and contract readiness |
| `GET` | `/api/v1/auth/config` | Auth runtime capabilities, trusted origins, and required production env |
| `GET\|POST` | `/api/auth/[...all]` | Better Auth route handler with 2FA and Next.js cookies |

## Rules

- Resource names are plural, lowercase, and kebab-case.
- Collections use cursor pagination once they can grow unbounded.
- Route handlers never expose upstream secrets or raw OAuth tokens.
- Validation failures use `422`; auth failures use `401`; upstream failures should map to `502` or `503`.
- SSE events include stable IDs and typed event names.

## Auth Runtime

The auth UI calls the Better Auth client directly. Better Auth owns email/password sign-in, sign-up, OAuth redirects, session cookies, and TOTP verification through `/api/auth/[...all]`.

Required runtime environment:

```text
BETTER_AUTH_SECRET
BETTER_AUTH_URL
DATABASE_URL
```

Run Better Auth schema migrations against the configured database before accepting live sessions:

```bash
pnpm exec auth migrate
```
