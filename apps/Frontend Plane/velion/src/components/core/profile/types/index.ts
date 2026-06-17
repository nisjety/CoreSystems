export interface UserProfile {
  id: string;
  authUserId?: string;
  email: string;
  name: string;
  avatar?: string | null;
  position?: string | null;
  department?: string | null;
  status: 'online' | 'away' | 'busy' | 'offline';
  emailVerified: boolean;
  lastLogin?: string | null;
  createdAt?: string;
}

export interface ProviderAccount {
  id: string;
  provider: string;
  providerUserId?: string;
  email?: string;
  displayName?: string;
  linkedAt?: string;
}

export interface UpdateProfilePayload {
  name?: string;
  avatar?: string;
  position?: string;
  department?: string;
  status?: UserProfile['status'];
}
