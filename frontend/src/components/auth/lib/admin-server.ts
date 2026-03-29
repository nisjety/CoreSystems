import { cookies } from 'next/headers';

// Backend URL configuration
const getBackendUrl = () => {
  return process.env.AUTH_SERVICE_URL || 'http://auth-service:3011';
};

/**
 * Server-side admin utilities
 * These functions make direct calls to the Better Auth admin API endpoints
 */

export interface AdminCreateUserParams {
  email: string;
  password: string;
  name: string;
  role?: string;
  data?: Record<string, any>;
}

export interface AdminListUsersParams {
  limit?: number;
  offset?: number;
  searchValue?: string;
  searchField?: 'email' | 'name';
  searchOperator?: 'contains' | 'starts_with' | 'ends_with';
  sortBy?: string;
  sortDirection?: 'asc' | 'desc';
  filterField?: string;
  filterValue?: string | number | boolean;
  filterOperator?: 'eq' | 'ne' | 'lt' | 'lte' | 'gt' | 'gte';
}

export interface AdminUser {
  id: string;
  email: string;
  name: string;
  role?: string;
  banned?: boolean;
  banReason?: string;
  banExpires?: string;
  emailVerified?: boolean;
  createdAt?: string;
  updatedAt?: string;
}

export interface AdminListUsersResponse {
  users: AdminUser[];
  total: number;
  limit?: number;
  offset?: number;
}

/**
 * Create a new user via Better Auth admin API
 */
export async function adminCreateUser(params: AdminCreateUserParams): Promise<{ user?: AdminUser; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    const response = await fetch(`${BACKEND_URL}/api/auth/admin/create-user`, {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(params),
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin create user failed:', response.status, errorText);
      return { error: `Failed to create user: ${errorText}` };
    }

    const data = await response.json();
    return { user: data };
  } catch (error) {
    console.error('Error creating user:', error);
    return { error: 'Failed to create user' };
  }
}

/**
 * List users via Better Auth admin API
 */
export async function adminListUsers(params: AdminListUsersParams = {}): Promise<{ data?: AdminListUsersResponse; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    
    // Build query parameters
    const queryParams = new URLSearchParams();
    Object.entries(params).forEach(([key, value]) => {
      if (value !== undefined && value !== null) {
        queryParams.append(key, String(value));
      }
    });
    
    const url = `${BACKEND_URL}/api/auth/admin/list-users${queryParams.toString() ? '?' + queryParams.toString() : ''}`;
    
    const response = await fetch(url, {
      method: 'GET',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin list users failed:', response.status, errorText);
      return { error: `Failed to list users: ${errorText}` };
    }

    const data = await response.json();
    return { data };
  } catch (error) {
    console.error('Error listing users:', error);
    return { error: 'Failed to list users' };
  }
}

/**
 * Set user role via Better Auth admin API
 */
export async function adminSetRole(userId: string, role: string | string[]): Promise<{ success?: boolean; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    const response = await fetch(`${BACKEND_URL}/api/auth/admin/set-role`, {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ userId, role }),
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin set role failed:', response.status, errorText);
      return { error: `Failed to set role: ${errorText}` };
    }

    return { success: true };
  } catch (error) {
    console.error('Error setting role:', error);
    return { error: 'Failed to set role' };
  }
}

/**
 * Ban user via Better Auth admin API
 */
export async function adminBanUser(userId: string, banReason?: string, banExpiresIn?: number): Promise<{ success?: boolean; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    const response = await fetch(`${BACKEND_URL}/api/auth/admin/ban-user`, {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ userId, banReason, banExpiresIn }),
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin ban user failed:', response.status, errorText);
      return { error: `Failed to ban user: ${errorText}` };
    }

    return { success: true };
  } catch (error) {
    console.error('Error banning user:', error);
    return { error: 'Failed to ban user' };
  }
}

/**
 * Unban user via Better Auth admin API
 */
export async function adminUnbanUser(userId: string): Promise<{ success?: boolean; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    const response = await fetch(`${BACKEND_URL}/api/auth/admin/unban-user`, {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ userId }),
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin unban user failed:', response.status, errorText);
      return { error: `Failed to unban user: ${errorText}` };
    }

    return { success: true };
  } catch (error) {
    console.error('Error unbanning user:', error);
    return { error: 'Failed to unban user' };
  }
}

/**
 * Remove user via Better Auth admin API
 */
export async function adminRemoveUser(userId: string): Promise<{ success?: boolean; error?: string }> {
  try {
    const cookieStore = await cookies();
    const cookieHeader = cookieStore.toString();
    
    const BACKEND_URL = getBackendUrl();
    const response = await fetch(`${BACKEND_URL}/api/auth/admin/remove-user`, {
      method: 'POST',
      headers: {
        'Cookie': cookieHeader,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ userId }),
      cache: 'no-store',
    });

    if (!response.ok) {
      const errorText = await response.text();
      console.error('Admin remove user failed:', response.status, errorText);
      return { error: `Failed to remove user: ${errorText}` };
    }

    return { success: true };
  } catch (error) {
    console.error('Error removing user:', error);
    return { error: 'Failed to remove user' };
  }
}

/**
 * Get system stats (placeholder for now)
 */
export async function adminGetSystemStats(): Promise<{ stats?: any; error?: string }> {
  try {
    // For now, return mock stats since Better Auth doesn't have this endpoint
    // You can implement this by querying the database directly if needed
    const stats = {
      totalUsers: 0,
      activeUsers: 0,
      totalOrganizations: 0,
      totalApiKeys: 0,
      activeSessions: 0,
      recentSignUps: 0,
      recentLogins: 0
    };
    
    return { stats };
  } catch (error) {
    console.error('Error getting system stats:', error);
    return { error: 'Failed to get system stats' };
  }
}