'use client';

import { useState } from 'react';

interface User {
  id: string;
  email: string;
  name: string;
  emailVerified: boolean;
}

interface UserServiceTesterProps {
  user: User;
}

interface ApiResponse {
  success: boolean;
  data?: any;
  error?: string;
  timestamp: string;
}

const USER_SERVICE_URL = 'http://localhost:3012';

export default function UserServiceTester({ user }: UserServiceTesterProps) {
  const [results, setResults] = useState<ApiResponse[]>([]);
  const [loading, setLoading] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [updateData, setUpdateData] = useState('{"name": "Updated Name"}');

  const addResult = (result: ApiResponse) => {
    setResults(prev => [result, ...prev]);
  };

  const callUserService = async (endpoint: string, method = 'GET', body?: any) => {
    const testId = `${method} ${endpoint}`;
    setLoading(testId);
    
    try {
      const options: RequestInit = {
        method,
        headers: {
          'Content-Type': 'application/json',
          'X-API-Key': 'auth-service-internal-key-dev-2024', // Internal API key for user-service
        },
      };

      if (body) {
        options.body = JSON.stringify(body);
      }

      const response = await fetch(`${USER_SERVICE_URL}${endpoint}`, options);
      const data = await response.json();

      addResult({
        success: response.ok,
        data: response.ok ? data : undefined,
        error: response.ok ? undefined : `${response.status}: ${data?.message || 'Unknown error'}`,
        timestamp: new Date().toISOString(),
      });
    } catch (error) {
      addResult({
        success: false,
        error: error instanceof Error ? error.message : 'Network error',
        timestamp: new Date().toISOString(),
      });
    } finally {
      setLoading(null);
    }
  };

  const tests = [
    {
      name: 'Get Current User by ID',
      description: 'Test fetching current user data using their ID',
      action: () => callUserService(`/api/v1/users/${user.id}`),
    },
    {
      name: 'Get Current User by Email',
      description: 'Test fetching current user data using their email',
      action: () => callUserService(`/api/v1/users/by-email/${encodeURIComponent(user.email)}`),
    },
    {
      name: 'Get Current User Profile',
      description: 'Test fetching current user profile using email query parameter',
      action: () => callUserService(`/api/v1/users/me/profile?email=${encodeURIComponent(user.email)}`),
    },
    {
      name: 'Get User Sessions',
      description: 'Test fetching active sessions for current user',
      action: () => callUserService(`/api/v1/users/${user.id}/sessions`),
    },
    {
      name: 'Search Users',
      description: 'Test searching users with query parameter',
      action: () => callUserService(`/api/v1/users/search?q=${encodeURIComponent(searchQuery)}&limit=10`),
      requiresInput: true,
    },
    {
      name: 'Update User Profile',
      description: 'Test updating user profile data',
      action: () => {
        try {
          const profileData = JSON.parse(updateData);
          return callUserService(`/api/v1/users/me/profile?email=${encodeURIComponent(user.email)}`, 'PUT', profileData);
        } catch (error) {
          addResult({
            success: false,
            error: 'Invalid JSON in update data',
            timestamp: new Date().toISOString(),
          });
        }
      },
      requiresInput: true,
    },
  ];

  const clearResults = () => setResults([]);

  return (
    <div className="space-y-8">
      <div className="bg-card rounded-lg border border-border">
        <div className="p-6 border-b border-border">
          <h3 className="text-lg font-semibold text-foreground">User Service API Testing</h3>
          <p className="text-sm text-muted-foreground mt-1">
            Test various endpoints of the user-service (running on port 3012) using the current authenticated user.
          </p>
        </div>
        <div className="p-6 space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label htmlFor="tester-search-query" className="text-sm font-medium text-foreground">Search Query:</label>
              <input
                id="tester-search-query"
                type="text"
                value={searchQuery} 
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Enter search term for user search test"
                className="mt-1 w-full px-3 py-2 border border-border rounded-md bg-background text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
            </div>
            <div>
              <label htmlFor="tester-update-data" className="text-sm font-medium text-foreground">Update Data (JSON):</label>
              <textarea
                id="tester-update-data"
                value={updateData} 
                onChange={(e) => setUpdateData(e.target.value)}
                placeholder='{"name": "New Name", "bio": "Updated bio"}'
                className="mt-1 w-full px-3 py-2 border border-border rounded-md bg-background text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-blue-500 h-20 resize-none"
              />
            </div>
          </div>
          
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {tests.map((test, index) => (
              <div key={test.name} className="bg-card border border-border rounded-lg">
                <div className="p-4 border-b border-border">
                  <h4 className="text-sm font-semibold text-foreground">{test.name}</h4>
                  <p className="text-xs text-muted-foreground mt-1">{test.description}</p>
                </div>
                <div className="p-4">
                  <button
                    onClick={test.action}
                    disabled={loading !== null || (test.requiresInput && (
                      (test.name.includes('Search') && !searchQuery) ||
                      (test.name.includes('Update') && !updateData)
                    ))}
                    className="w-full px-3 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:bg-gray-400 disabled:cursor-not-allowed text-sm transition-colors"
                  >
                    {loading ? 'Testing...' : 'Test'}
                  </button>
                </div>
              </div>
            ))}
          </div>

          <div className="flex justify-center">
            <button
              onClick={clearResults}
              disabled={results.length === 0}
              className="px-4 py-2 border border-border rounded-md bg-background text-foreground hover:bg-gray-50 disabled:opacity-50 disabled:cursor-not-allowed text-sm transition-colors"
            >
              Clear Results
            </button>
          </div>
        </div>
      </div>

      {results.length > 0 && (
        <div className="bg-card rounded-lg border border-border">
          <div className="p-6 border-b border-border">
            <h3 className="text-lg font-semibold text-foreground">Test Results</h3>
            <p className="text-sm text-muted-foreground mt-1">Latest results first</p>
          </div>
          <div className="p-6">
            <div className="space-y-4 max-h-96 overflow-y-auto">
              {results.map((result, index) => (
                <div key={result.timestamp} className="border border-border rounded-lg p-4">
                  <div className="flex items-center justify-between mb-2">
                    <span className={`inline-flex items-center px-2 py-1 rounded-full text-xs font-medium ${
                      result.success 
                        ? 'bg-green-100 text-green-800' 
                        : 'bg-red-100 text-red-800'
                    }`}>
                      {result.success ? 'Success' : 'Error'}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {new Date(result.timestamp).toLocaleTimeString()}
                    </span>
                  </div>
                  
                  {result.error && (
                    <div className="text-sm text-red-600 mb-2">
                      <strong>Error:</strong> {result.error}
                    </div>
                  )}
                  
                  {result.data && (
                    <div className="text-sm">
                      <strong>Response:</strong>
                      <pre className="mt-1 bg-gray-100 p-2 rounded text-xs overflow-x-auto border">
                        {JSON.stringify(result.data, null, 2)}
                      </pre>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      <div className="bg-card rounded-lg border border-border">
        <div className="p-6 border-b border-border">
          <h3 className="text-lg font-semibold text-foreground">Service Information</h3>
        </div>
        <div className="p-6">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-muted-foreground">User Service URL:</span>
              <code className="ml-2 bg-gray-100 px-2 py-1 rounded text-xs border">{USER_SERVICE_URL}</code>
            </div>
            <div>
              <span className="text-muted-foreground">Test User ID:</span>
              <code className="ml-2 bg-gray-100 px-2 py-1 rounded text-xs border break-all">{user.id}</code>
            </div>
            <div>
              <span className="text-muted-foreground">Test User Email:</span>
              <code className="ml-2 bg-gray-100 px-2 py-1 rounded text-xs border break-all">{user.email}</code>
            </div>
            <div>
              <span className="text-muted-foreground">Available Endpoints:</span>
              <div className="ml-2 text-xs space-y-1 mt-1 font-mono">
                <div className="bg-gray-100 px-2 py-1 rounded border">GET /api/v1/users/:id</div>
                <div className="bg-gray-100 px-2 py-1 rounded border">GET /api/v1/users/by-email/:email</div>
                <div className="bg-gray-100 px-2 py-1 rounded border">GET /api/v1/users/me/profile</div>
                <div className="bg-gray-100 px-2 py-1 rounded border">PUT /api/v1/users/me/profile</div>
                <div className="bg-gray-100 px-2 py-1 rounded border">GET /api/v1/users/:id/sessions</div>
                <div className="bg-gray-100 px-2 py-1 rounded border">GET /api/v1/users/search</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}