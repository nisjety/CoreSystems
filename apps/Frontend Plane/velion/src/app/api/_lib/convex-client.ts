import { ConvexHttpClient } from 'convex/browser'

const CONVEX_API_URL =
  process.env.CONVEX_API_URL ||
  process.env.NEXT_PUBLIC_CONVEX_HTTP ||
  process.env.NEXT_PUBLIC_CONVEX_URL ||
  'http://127.0.0.1:3210'

const CONVEX_ADMIN_KEY =
  process.env.CONVEX_ADMIN_KEY ||
  process.env.CONVEX_SELF_HOSTED_ADMIN_KEY ||
  ''

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication',
  )
}

export const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY || INTERNAL_API_KEY

export function getConvexClient() {
  const client = new ConvexHttpClient(CONVEX_API_URL, {
    skipConvexDeploymentUrlCheck: true,
  })
  const unsafeClient = client as ConvexHttpClient & {
    setAdminAuth?: (token: string) => void
    setFetchOptions?: (options: { cache: 'force-cache' | 'no-store' }) => void
  }

  if (CONVEX_ADMIN_KEY) {
    unsafeClient.setAdminAuth?.(CONVEX_ADMIN_KEY)
  }

  unsafeClient.setFetchOptions?.({ cache: 'no-store' })
  return client
}

export async function convexQuery<T>(
  name: string,
  args: Record<string, unknown>,
): Promise<T> {
  return (await (getConvexClient() as any).query(name, {
    serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
    ...args,
  })) as T
}

export async function convexMutation<T>(
  name: string,
  args: Record<string, unknown>,
): Promise<T> {
  return (await (getConvexClient() as any).mutation(
    name,
    {
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      ...args,
    },
    { skipQueue: true },
  )) as T
}
