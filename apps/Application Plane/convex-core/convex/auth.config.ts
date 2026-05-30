const config = {
  providers: [
    {
      type: 'customJwt',
      issuer:
        process.env.CONVEX_AUTH_ISSUER ||
        'http://localhost:3011/api/convex-auth',
      jwks:
        process.env.CONVEX_AUTH_JWKS_URL ||
        // `auth-core` is the canonical service name (velion-gap.md G7).
        'http://auth-core:3011/api/convex-auth/jwks',
      applicationID:
        process.env.CONVEX_AUTH_AUDIENCE || 'coresystem-convex',
      algorithm: 'RS256',
    },
  ],
};

export default config;
