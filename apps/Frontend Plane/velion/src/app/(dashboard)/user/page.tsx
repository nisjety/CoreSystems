import Link from 'next/link';
import { requireAuth } from '@/components/auth/lib/auth-server';
import { SignOutButton } from '@/components/auth/ui/sign-out-button';
import UserServiceTester from './user-service-tester';

export default async function UserPage() {
  // Server-side session validation - this will redirect to /sign-in if not authenticated
  const session = await requireAuth();
  const { user } = session;

  return (
    <div className="h-full overflow-y-auto bg-background">
      <header className="border-b border-border bg-card">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between items-center h-16">
            <div className="flex items-center gap-4">
              <Link 
                href="/dashboard"
                className="px-3 py-1.5 text-sm bg-gray-200 text-gray-700 hover:bg-gray-300 rounded-md transition-colors"
              >
                ← Dashboard
              </Link>
              <h1 className="text-xl font-semibold text-foreground">User Service Testing</h1>
            </div>
            <div className="flex items-center gap-4">
              <div className="text-sm text-muted-foreground">
                Welcome, {user.name}
              </div>
              <SignOutButton />
            </div>
          </div>
        </div>
      </header>

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        <div className="bg-card rounded-lg border border-border p-6 mb-8">
          <h2 className="text-lg font-semibold mb-4">Current Session User</h2>
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground">User ID:</span>
                <code className="ml-2 bg-muted px-2 py-1 rounded">{user.id}</code>
              </div>
              <div>
                <span className="text-muted-foreground">Email:</span>
                <span className="ml-2">{user.email}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Name:</span>
                <span className="ml-2">{user.name}</span>
              </div>
              <div>
                <span className="text-muted-foreground">Email Verified:</span>
                <span className={`ml-2 inline-flex items-center px-2 py-1 rounded-full text-xs font-medium ${
                  user.emailVerified 
                    ? 'bg-green-100 text-green-800' 
                    : 'bg-yellow-100 text-yellow-800'
                }`}>
                  {user.emailVerified ? 'Yes' : 'No'}
                </span>
              </div>
            </div>
          </div>
        </div>

        <UserServiceTester user={user} />
      </main>
    </div>
  );
}