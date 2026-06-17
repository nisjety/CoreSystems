/**
 * Profile Service
 *
 * API client for user profile data from the user-service.
 * Routes go through Next.js proxy: /api/user/[...path] → user-service /api/v1/
 */

import type { UserProfile, ProviderAccount, UpdateProfilePayload } from '../types';
import { authService } from '@/components/auth/services/auth-service';

type UserServiceEnvelope = {
  user?: Record<string, unknown>;
};

type SessionContext = {
  userId: string
  orgId?: string
  role?: string
  onboardingStatus: string
}

class ProfileServiceAPI {
  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await fetch(path, {
      ...init,
      credentials: 'include',
      headers: {
        'Content-Type': 'application/json',
        ...(init.headers as Record<string, string>),
      },
    });

    if (!res.ok) {
      const err = new Error(`Request failed: ${res.status}`) as Error & { status?: number };
      err.status = res.status;
      throw err;
    }

    return res.json() as Promise<T>;
  }

  private toProfile(payload: unknown): UserProfile | null {
    const raw = (payload && typeof payload === 'object' && 'user' in (payload as UserServiceEnvelope))
      ? (payload as UserServiceEnvelope).user
      : payload;

    if (!raw || typeof raw !== 'object') {
      return null;
    }

    const data = raw as Record<string, unknown>;
    const id = typeof data.id === 'string' ? data.id : '';
    const email = typeof data.email === 'string' ? data.email : '';
    if (!id || !email) {
      return null;
    }

    const status = typeof data.status === 'string' ? data.status : 'online';
    const normalizedStatus: UserProfile['status'] =
      status === 'away' || status === 'busy' || status === 'offline' ? status : 'online';

    return {
      id,
      email,
      name: typeof data.name === 'string' ? data.name : '',
      avatar: typeof data.avatar === 'string' && data.avatar.length > 0 ? data.avatar : null,
      position: typeof data.position === 'string' ? data.position : null,
      department: typeof data.department === 'string' ? data.department : null,
      status: normalizedStatus,
      emailVerified: Boolean(data.email_verified),
      createdAt: typeof data.created_at === 'string' ? data.created_at : undefined,
      lastLogin: typeof data.last_login_at === 'string' ? data.last_login_at : null,
    };
  }

  private async getAuthHeaders(): Promise<Record<string, string>> {
    const user = await authService.getCurrentUser();
    const headers: Record<string, string> = {};

    if (user?.id) {
      headers['X-User-Id'] = user.id;
    }
    if (user?.email) {
      headers['X-User-Email'] = user.email;
    }
    if (user?.name) {
      headers['X-User-Name'] = user.name;
    }
    if (user?.image) {
      headers['X-User-Avatar'] = user.image;
    }

    return headers;
  }

  /** Full user profile from user-service (name, position, department, status …) */
  async getCurrentUser(): Promise<UserProfile | null> {
    const authHeaders = await this.getAuthHeaders();
    const authAvatar = authHeaders['X-User-Avatar'];
    const email = authHeaders['X-User-Email'];
    const query = email ? `?email=${encodeURIComponent(email)}` : '';

    try {
      const payload = await this.request<unknown>(`/api/user/users/current${query}`, {
        headers: authHeaders,
      });
      const normalized = this.toProfile(payload);
      if (!normalized) {
        return null;
      }

      const hasPlaceholderAvatar =
        !normalized.avatar ||
        normalized.avatar.includes('example.com/avatar') ||
        normalized.avatar.toLowerCase().includes('placeholder');

      if (hasPlaceholderAvatar && authAvatar) {
        return { ...normalized, avatar: authAvatar };
      }

      return normalized;
    } catch {
      // Fall back to auth-session data (lighter, always available)
      try {
        const payload = await this.request<unknown>('/api/user/current', {
          headers: authHeaders,
        });
        const normalized = this.toProfile(payload);
        if (!normalized) {
          return null;
        }

        const hasPlaceholderAvatar =
          !normalized.avatar ||
          normalized.avatar.includes('example.com/avatar') ||
          normalized.avatar.toLowerCase().includes('placeholder');

        if (hasPlaceholderAvatar && authAvatar) {
          return { ...normalized, avatar: authAvatar };
        }

        return normalized;
      } catch {
        return null;
      }
    }
  }

  /** Update editable profile fields (name, position, department, avatar, status) */
  async updateProfile(payload: UpdateProfilePayload): Promise<UserProfile> {
    const authHeaders = await this.getAuthHeaders();
    const response = await this.request<unknown>('/api/user/users/me', {
      method: 'PATCH',
      headers: authHeaders,
      body: JSON.stringify(payload),
    });

    const normalized = this.toProfile(response);
    if (!normalized) {
      throw new Error('Invalid profile response');
    }
    return normalized;
  }

  /** Linked OAuth provider accounts from user-service provider_accounts table */
  async getLinkedProviders(): Promise<ProviderAccount[]> {
    try {
      const payload = await this.request<unknown>('/api/user/providers');
      if (Array.isArray(payload)) {
        return payload as ProviderAccount[];
      }

      if (payload && typeof payload === 'object' && Array.isArray((payload as { providers?: unknown[] }).providers)) {
        return (payload as { providers: ProviderAccount[] }).providers;
      }

      return [];
    } catch {
      return [];
    }
  }

  async getSessionContext(): Promise<SessionContext | null> {
    try {
      return await this.request<SessionContext>('/api/user/me/session-context');
    } catch {
      return null;
    }
  }
}

export const profileService = new ProfileServiceAPI();
