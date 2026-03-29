// Auth service client for Microsoft Entra integration
export interface AuthUser {
  id: string;
  entraId: string;
  email: string;
  displayName: string;
  givenName?: string;
  surname?: string;
}

export interface LoginResponse {
  access_token: string;
  refresh_token?: string;
  token_type: 'Bearer';
  expires_in: number;
  session_id?: string;
  user: AuthUser;
}

export interface AuthTokens {
  accessToken: string;
  refreshToken?: string;
  expiresAt: number;
}

export interface AuthError {
  error: string;
  message: string;
  statusCode?: number;
}

class AuthServiceClient {
  private baseURL: string;
  private tokens: AuthTokens | null = null;

  constructor(baseURL = process.env.NEXT_PUBLIC_AUTH_SERVICE_URL || 'http://localhost:3011') {
    this.baseURL = baseURL;
    this.loadTokensFromStorage();
  }

  private loadTokensFromStorage() {
    if (typeof window === 'undefined') return;
    
    try {
      const stored = localStorage.getItem('auth_tokens');
      if (stored) {
        this.tokens = JSON.parse(stored);
        
        // Check if tokens are expired
        if (this.tokens && Date.now() >= this.tokens.expiresAt) {
          this.clearTokens();
        }
      }
    } catch (error) {
      console.warn('Failed to load tokens from storage:', error);
      this.clearTokens();
    }
  }

  private saveTokensToStorage(tokens: AuthTokens) {
    if (typeof window === 'undefined') return;
    
    try {
      localStorage.setItem('auth_tokens', JSON.stringify(tokens));
      this.tokens = tokens;
    } catch (error) {
      console.warn('Failed to save tokens to storage:', error);
    }
  }

  private clearTokens() {
    if (typeof window === 'undefined') return;
    
    try {
      localStorage.removeItem('auth_tokens');
      localStorage.removeItem('auth_user');
      localStorage.removeItem('auth_session_id');
      this.tokens = null;
    } catch (error) {
      console.warn('Failed to clear tokens from storage:', error);
    }
  }

  private async request<T>(
    endpoint: string, 
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseURL}/api/auth${endpoint}`;
    
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    };
    
    if (this.tokens?.accessToken) {
      headers['Authorization'] = `Bearer ${this.tokens.accessToken}`;
    }    const response = await fetch(url, {
      ...options,
      headers,
    });

    if (!response.ok) {
      let errorData: AuthError;
      try {
        errorData = await response.json();
      } catch {
        errorData = {
          error: 'Request Failed',
          message: `HTTP ${response.status}: ${response.statusText}`,
          statusCode: response.status,
        };
      }
      throw new Error(errorData.message || `Authentication request failed: ${response.status}`);
    }

    return response.json();
  }

  // Initiate Microsoft Entra login
  async initiateLogin(): Promise<void> {
    // Redirect to the auth service login endpoint
    const loginUrl = `${this.baseURL}/api/auth/login`;
    window.location.href = loginUrl;
  }

  // Handle the callback from Microsoft Entra
  async handleCallback(code: string, state?: string): Promise<LoginResponse> {
    const response = await this.request<LoginResponse>('/callback', {
      method: 'POST',
      body: JSON.stringify({ code, state }),
    });

    // Store tokens and user info
    if (response.access_token) {
      const tokens: AuthTokens = {
        accessToken: response.access_token,
        refreshToken: response.refresh_token,
        expiresAt: Date.now() + (response.expires_in * 1000),
      };
      
      this.saveTokensToStorage(tokens);
      
      // Store user and session info
      if (typeof window !== 'undefined') {
        try {
          localStorage.setItem('auth_user', JSON.stringify(response.user));
          if (response.session_id) {
            localStorage.setItem('auth_session_id', response.session_id);
          }
        } catch (error) {
          console.warn('Failed to store user data:', error);
        }
      }
    }

    return response;
  }

  // Get current user profile
  async getProfile(): Promise<AuthUser | null> {
    try {
      // First try to get from localStorage
      if (typeof window !== 'undefined') {
        const stored = localStorage.getItem('auth_user');
        if (stored) {
          const user = JSON.parse(stored);
          // Verify with server if we have tokens
          if (this.tokens?.accessToken) {
            try {
              const serverUser = await this.request<AuthUser>('/profile');
              // Update stored user with server data
              localStorage.setItem('auth_user', JSON.stringify(serverUser));
              return serverUser;
            } catch (error) {
              console.warn('Failed to verify user with server:', error);
              // Return stored user if server request fails
              return user;
            }
          }
          return user;
        }
      }

      // If no stored user and we have tokens, fetch from server
      if (this.tokens?.accessToken) {
        const user = await this.request<AuthUser>('/profile');
        if (typeof window !== 'undefined') {
          localStorage.setItem('auth_user', JSON.stringify(user));
        }
        return user;
      }

      return null;
    } catch (error) {
      console.warn('Failed to get user profile:', error);
      return null;
    }
  }

  // Refresh access token
  async refreshToken(): Promise<boolean> {
    if (!this.tokens?.refreshToken) {
      return false;
    }

    try {
      const response = await this.request<{ access_token: string; expires_in: number }>('/refresh', {
        method: 'POST',
        body: JSON.stringify({ refresh_token: this.tokens.refreshToken }),
      });

      const newTokens: AuthTokens = {
        accessToken: response.access_token,
        refreshToken: this.tokens.refreshToken, // Keep the same refresh token
        expiresAt: Date.now() + (response.expires_in * 1000),
      };

      this.saveTokensToStorage(newTokens);
      return true;
    } catch (error) {
      console.warn('Failed to refresh token:', error);
      this.clearTokens();
      return false;
    }
  }

  // Check if user is authenticated
  isAuthenticated(): boolean {
    if (!this.tokens) return false;
    
    // Check if tokens are expired
    if (Date.now() >= this.tokens.expiresAt) {
      // Try to refresh token if we have a refresh token
      if (this.tokens.refreshToken) {
        this.refreshToken().catch(() => {
          this.clearTokens();
        });
      } else {
        this.clearTokens();
      }
      return false;
    }
    
    return true;
  }

  // Get access token (with automatic refresh)
  async getAccessToken(): Promise<string | null> {
    if (!this.tokens) return null;
    
    // Check if token is about to expire (refresh 5 minutes before expiry)
    const fiveMinutes = 5 * 60 * 1000;
    if (Date.now() >= (this.tokens.expiresAt - fiveMinutes)) {
      if (this.tokens.refreshToken) {
        const refreshed = await this.refreshToken();
        if (!refreshed) return null;
      } else {
        this.clearTokens();
        return null;
      }
    }
    
    return this.tokens.accessToken;
  }

  // Sign out
  async signOut(): Promise<void> {
    try {
      // Get session ID for server-side logout
      const sessionId = typeof window !== 'undefined' 
        ? localStorage.getItem('auth_session_id') 
        : null;

      if (this.tokens?.accessToken) {
        await this.request('/logout', {
          method: 'POST',
        });
      }

      // Clear all local data
      this.clearTokens();

      // Get Microsoft logout URL for complete logout
      const logoutResponse = await fetch(`${this.baseURL}/api/auth/logout`, {
        method: 'POST',
      });
      
      if (logoutResponse.ok) {
        const data = await logoutResponse.json();
        if (data.logout_url) {
          // Redirect to Microsoft logout
          window.location.href = data.logout_url;
          return;
        }
      }
    } catch (error) {
      console.warn('Logout error:', error);
    }
    
    // Fallback: just clear local data and redirect to home
    this.clearTokens();
    if (typeof window !== 'undefined') {
      window.location.href = '/';
    }
  }

  // Validate session
  async validateSession(): Promise<boolean> {
    try {
      const sessionId = typeof window !== 'undefined' 
        ? localStorage.getItem('auth_session_id') 
        : null;

      if (!sessionId || !this.tokens?.accessToken) {
        return false;
      }

      const response = await this.request<{ valid: boolean }>(`/session/${sessionId}/validate`);
      return response.valid;
    } catch (error) {
      console.warn('Session validation failed:', error);
      return false;
    }
  }

  // Get service health status
  async getHealth(): Promise<{ status: string; timestamp?: string; message?: string }> {
    try {
      return await this.request('/health');
    } catch (error) {
      console.warn('Health check failed:', error);
      return { status: 'error', message: 'Service unavailable' };
    }
  }
}

// Create a singleton instance
export const authServiceClient = new AuthServiceClient();

// Export the class for testing or custom instances
export { AuthServiceClient };