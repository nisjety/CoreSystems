export async function register() {
  if (process.env.NEXT_RUNTIME === 'nodejs') {
    await import('./src/components/auth/lib/orpc/orpc.server');
  }
}
