// API client for making HTTP requests to backend services

class APIClient {
  private baseURL: string
  private timeoutMs: number

  constructor(baseURL = '', timeoutMs = 15_000) {
    this.baseURL = baseURL
    this.timeoutMs = timeoutMs
  }

  private async request<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseURL}${endpoint}`
    // Endpoints frequently probed during the OAuth-callback retry window
    // (see verevon-gap.md G30). Per-attempt errors are expected until the
    // Better Auth cookie lands; downgrade their console output from `error`
    // to a single `warn` so dev traces don't look like fires.
    const suppressOnboardingProbeLogs =
      endpoint.includes('/api/user/current') ||
      endpoint.includes('/api/user/me/session-context') ||
      endpoint.includes('/api/user/onboarding/complete') ||
      endpoint.includes('/api/org/orgs/me')
    
    const defaultHeaders: Record<string, string> = {
      'Content-Type': 'application/json',
    }

    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), this.timeoutMs)
    const externalSignal = options.signal
    if (externalSignal) {
      if (externalSignal.aborted) {
        controller.abort()
      } else {
        externalSignal.addEventListener('abort', () => controller.abort(), { once: true })
      }
    }

    const config: RequestInit = {
      ...options,
      credentials: 'include',
      signal: controller.signal,
      headers: {
        ...defaultHeaders,
        ...options.headers,
      },
    }

    try {
      const response = await fetch(url, config)

      if (!response.ok) {
        let error: any = { message: response.statusText };
        try {
          error = await response.json();
        } catch {
          // Response is not JSON, use status text
          error.message = `${response.status} ${response.statusText}`;
        }
        
        const errorMessage = error.message || error.error || 'Request failed';
        throw new Error(errorMessage);
      }

      // Handle 204 No Content
      if (response.status === 204) {
        return {} as T
      }

      return await response.json()
    } catch (error) {
      const normalizedError =
        error instanceof DOMException && error.name === 'AbortError'
          ? new Error(`Request timed out after ${this.timeoutMs}ms`)
          : error
      if (suppressOnboardingProbeLogs) {
        console.warn(`API request failed: ${endpoint}`)
      } else {
        console.error(`API request failed: ${endpoint}`, normalizedError)
      }
      throw normalizedError
    } finally {
      clearTimeout(timeout)
    }
  }

  async get<T>(endpoint: string, options?: RequestInit): Promise<T> {
    return this.request<T>(endpoint, {
      ...options,
      method: 'GET',
    })
  }

  async post<T>(
    endpoint: string,
    data?: any,
    options?: RequestInit
  ): Promise<T> {
    return this.request<T>(endpoint, {
      ...options,
      method: 'POST',
      body: data ? JSON.stringify(data) : undefined,
    })
  }

  async put<T>(
    endpoint: string,
    data?: any,
    options?: RequestInit
  ): Promise<T> {
    return this.request<T>(endpoint, {
      ...options,
      method: 'PUT',
      body: data ? JSON.stringify(data) : undefined,
    })
  }

  async patch<T>(
    endpoint: string,
    data?: any,
    options?: RequestInit
  ): Promise<T> {
    return this.request<T>(endpoint, {
      ...options,
      method: 'PATCH',
      body: data ? JSON.stringify(data) : undefined,
    })
  }

  async delete<T>(endpoint: string, options?: RequestInit): Promise<T> {
    return this.request<T>(endpoint, {
      ...options,
      method: 'DELETE',
    })
  }
}

// Export singleton instance
export const apiClient = new APIClient()
