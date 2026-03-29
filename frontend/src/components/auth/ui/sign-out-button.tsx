'use client';

import { useRouter } from 'next/navigation';
import { useAuth } from '../hooks/use-auth';
import { Button } from '../ui/button';

export function SignOutButton() {
  const router = useRouter();
  const { signOut } = useAuth();

  const handleSignOut = async () => {
    try {
      await signOut();
      router.push('/sign-in');
    } catch (error) {
      console.error('Sign out failed:', error);
    }
  };

  return (
    <Button 
      onClick={handleSignOut}
      variant="outline"
      className="text-sm"
    >
      Sign Out
    </Button>
  );
}
