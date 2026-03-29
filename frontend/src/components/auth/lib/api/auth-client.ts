import { authClient } from '../auth-client';

// Type-safe API client using Better Auth as base
export class ApiClient {
  private baseURL: string;

  constructor() {
    if (typeof window !== 'undefined') {
      this.baseURL = `${window.location.origin}/api`;
      return;
    }

    const serverUrl =
      process.env.API_AUTH_URL ||
      process.env.NEXT_PUBLIC_API_URL ||
      'http://auth-service:3011';
    this.baseURL = serverUrl.endsWith('/api') ? serverUrl : `${serverUrl}/api`;
  }

  private async makeRequest<T>(
    endpoint: string, 
    options: RequestInit = {}
  ): Promise<T> {
    // Get session token from Better Auth
    const session = await authClient.getSession();
    const token = session.data?.session?.token;

    const response = await fetch(`${this.baseURL}${endpoint}`, {
      ...options,
      headers: {
        'Content-Type': 'application/json',
        ...(token && { Authorization: `Bearer ${token}` }),
        ...options.headers,
      },
    });

    if (!response.ok) {
      throw new Error(`API Error: ${response.status} ${response.statusText}`);
    }

    return response.json() as T;
  }

  // User profile methods
  async getProfile() {
    return this.makeRequest<{
      id: string;
      email: string;
      name: string;
      emailVerified: boolean;
      createdAt: string;
      updatedAt: string;
    }>('/profile');
  }

  async updateProfile(data: { name?: string; email?: string }) {
    return this.makeRequest<{
      id: string;
      email: string;
      name: string;
      emailVerified: boolean;
      createdAt: string;
      updatedAt: string;
    }>('/profile', {
      method: 'PUT',
      body: JSON.stringify(data),
    });
  }

  // Account management
  async deleteAccount(password: string) {
    return this.makeRequest<{ success: boolean; message: string }>('/account', {
      method: 'DELETE',
      body: JSON.stringify({ password }),
    });
  }

  // 2FA management
  async enable2FA(method: 'email' | 'sms' | 'totp', phoneNumber?: string) {
    return this.makeRequest<{
      success: boolean;
      qrCode?: string;
      secret?: string;
    }>('/2fa/enable', {
      method: 'POST',
      body: JSON.stringify({ method, phoneNumber }),
    });
  }

  async disable2FA(password: string) {
    return this.makeRequest<{ success: boolean; message: string }>('/2fa/disable', {
      method: 'POST',
      body: JSON.stringify({ password }),
    });
  }
}

export const apiClient = new ApiClient();
