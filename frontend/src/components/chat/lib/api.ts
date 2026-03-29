import { io, Socket } from 'socket.io-client';

const API_BASE_URL = 'http://localhost:3016';

// Types
export interface WeatherData {
  temperature: number;
  humidity: number;
  windSpeed: number;
  windDirection: number;
  precipitation: number;
  pressure: number;
  condition: string;
  icon: string;
  location: string;
  lastUpdated: string;
}

export interface WeatherForecast {
  current: WeatherData;
  forecast: Array<{
    date: string;
    temperature: {
      min: number;
      max: number;
    };
    condition: string;
    icon: string;
    precipitation: number;
  }>;
}

export interface NewsItem {
  id: string;
  title: string;
  description: string;
  link: string;
  publishDate: string;
  source: string;
  category?: string;
  imageUrl?: string;
}

export interface NewsResponse {
  articles: NewsItem[];
  lastUpdated: string;
  totalCount: number;
  hasMore?: boolean;
}

export interface AquatiqAd {
  id: string;
  title: string;
  description: string;
  imageUrl?: string;
  link?: string;
  category: string;
  priority: number;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AquatiqResponse {
  ads: AquatiqAd[];
  totalCount: number;
  lastUpdated: string;
}

export interface TrafficData {
  id: string;
  name: string;
  location: string;
  coordinates: {
    lat: number;
    lon: number;
  };
  trafficVolume?: number;
  averageSpeed?: number;
  status: 'operational' | 'maintenance' | 'offline';
  distance?: number; // Distance in km from user location
}

export interface TrafficResponse {
  success: boolean;
  data: TrafficData[];
  error?: string;
}

export interface AnalyticsEvent {
  eventType: 'page_view' | 'card_interaction' | 'api_call' | 'notification_click' | 'chat_message' | 'error';
  eventName: string;
  properties: Record<string, unknown>;
  sessionId: string;
  userId?: string;
}

export interface NotificationPayload {
  id: string;
  type: 'weather' | 'news' | 'aquatiq' | 'system';
  title: string;
  message: string;
  data?: unknown;
  timestamp: string;
  priority: 'low' | 'medium' | 'high';
  userId?: string;
}

// Realtime event payload minimal typing
type RealtimePayload = Record<string, unknown>;

class ApiClient {
  private socket: Socket | null = null;
  private sessionId: string;
  private userId?: string;

  constructor() {
    this.sessionId = this.generateSessionId();
    this.initializeSocket();
  }

  private generateSessionId(): string {
    return `session_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  private initializeSocket() {
    this.socket = io(API_BASE_URL, {
      autoConnect: false,
    });

    this.socket.on('connect', () => {
      console.log('Connected to notification server');
      if (this.userId) {
        this.socket?.emit('joinRoom', { userId: this.userId });
      }
    });

    this.socket.on('disconnect', () => {
      console.log('Disconnected from notification server');
    });

    this.socket.on('notification', (notification: NotificationPayload) => {
      this.handleNotification(notification);
    });

  this.socket.on('weatherUpdate', (data: RealtimePayload) => {
      this.handleRealtimeUpdate('weather', data);
    });

  this.socket.on('newsUpdate', (data: RealtimePayload) => {
      this.handleRealtimeUpdate('news', data);
    });

  this.socket.on('aquatiqUpdate', (data: RealtimePayload) => {
      this.handleRealtimeUpdate('aquatiq', data);
    });
  }

  // Initialize user session and connect socket
  setUserId(userId: string) {
    this.userId = userId;
    if (this.socket?.connected) {
      this.socket.emit('joinRoom', { userId });
    }
    if (!this.socket?.connected) {
      this.socket?.connect();
    }
    
    // Start analytics session
    this.trackEvent({
      eventType: 'page_view',
      eventName: 'session_start',
      properties: {
        userAgent: navigator.userAgent,
        viewport: `${window.innerWidth}x${window.innerHeight}`,
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      },
    });
  }

  // Disconnect socket and end session
  async disconnect() {
    if (this.userId) {
      this.socket?.emit('leaveRoom', { userId: this.userId });
    }
    this.socket?.disconnect();
    
    // End analytics session - use fetch directly to avoid infinite loop
    try {
      await fetch(`${API_BASE_URL}/analytics/session/end/${this.sessionId}`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
      });
  } catch {
      // Ignore session end errors
    }
  }

  private async request<T>(endpoint: string, method: 'GET' | 'POST' = 'GET', data?: unknown, timeout = 10000): Promise<T> {
    const startTime = performance.now();
    const controller = new AbortController();
    
    // Set up timeout
    const timeoutId = setTimeout(() => controller.abort(), timeout);
    
    try {
      const options: RequestInit = {
        method,
        headers: {
          'Content-Type': 'application/json',
        },
        signal: controller.signal,
      };

      if (data && method === 'POST') {
        options.body = JSON.stringify(data);
      }

      const response = await fetch(`${API_BASE_URL}${endpoint}`, options);
      clearTimeout(timeoutId);
      const responseTime = performance.now() - startTime;
      
            
      // Track API call (but exclude analytics endpoints to prevent infinite loops)
      if (!endpoint.startsWith('/analytics/')) {
        this.trackApiCall(endpoint, method, responseTime, response.status);
      }
      
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }
      
      return await response.json();
    } catch (err) {
      clearTimeout(timeoutId);
      const responseTime = performance.now() - startTime;
      
      // Handle specific error types
      if (err instanceof Error) {
        if (err.name === 'AbortError') {
          console.error(`Request timeout for ${endpoint} after ${timeout}ms`);
          throw new Error(`Request timeout: ${endpoint}`);
        }
        
        // Track API call failure (but exclude analytics endpoints to prevent infinite loops)
        if (!endpoint.startsWith('/analytics/')) {
          this.trackApiCall(endpoint, method, responseTime, 0);
          this.trackError(err.message, `API call to ${endpoint}`);
        }
        
        console.error(`API request failed for ${endpoint}:`, err);
        throw err;
      }
      
      // Handle unknown errors
      const errorMsg = 'Unknown error occurred';
      if (!endpoint.startsWith('/analytics/')) {
        this.trackApiCall(endpoint, method, responseTime, 0);
        this.trackError(errorMsg, `API call to ${endpoint}`);
      }
      
      console.error(`API request failed for ${endpoint}:`, err);
      throw new Error(errorMsg);
    }
  }

  // Analytics methods
  async trackEvent(event: Omit<AnalyticsEvent, 'sessionId'>) {
    try {
      // Use fetch directly to avoid infinite loop through request method
      await fetch(`${API_BASE_URL}/analytics/track`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          ...event,
          sessionId: this.sessionId,
          userId: this.userId,
        }),
      });
    } catch (_err) {
      console.error('Failed to track event:', _err);
    }
  }

  async trackCardInteraction(cardType: string, action: string) {
    try {
      // Use fetch directly to avoid infinite loop through request method
      await fetch(`${API_BASE_URL}/analytics/card-interaction`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          cardType,
          action,
          sessionId: this.sessionId,
          userId: this.userId,
        }),
      });
    } catch (_err) {
      console.error('Failed to track card interaction:', _err);
    }
  }

  private async trackApiCall(endpoint: string, method: string, responseTime: number, statusCode: number) {
    try {
      // Use fetch directly to avoid infinite loop through request method
      await fetch(`${API_BASE_URL}/analytics/api-call`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          endpoint,
          method,
          responseTime,
          statusCode,
          sessionId: this.sessionId,
        }),
      });
  } catch {
      // Don't log API tracking errors to avoid recursion
    }
  }

  private async trackError(error: string, context: string) {
    try {
      // Use fetch directly to avoid infinite loop through request method
      await fetch(`${API_BASE_URL}/analytics/error`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          error,
          context,
          sessionId: this.sessionId,
          userId: this.userId,
        }),
      });
    } catch (err) {
      console.error('Failed to track error:', err);
    }
  }

  // Notification handlers
  private handleNotification(notification: NotificationPayload) {
    // Dispatch custom event for components to listen to
    window.dispatchEvent(new CustomEvent('aquatiq-notification', {
      detail: notification
    }));

    // Show browser notification if supported and permitted
    if ('Notification' in window && Notification.permission === 'granted') {
      new Notification(notification.title, {
        body: notification.message,
        icon: '/favicon.ico',
        tag: notification.id,
      });
    }
  }

  private handleRealtimeUpdate(type: string, data: RealtimePayload) {
    // Dispatch custom event for real-time updates
    window.dispatchEvent(new CustomEvent(`aquatiq-${type}-update`, {
      detail: data
    }));
  }

  // Weather API methods
  async getWeatherForecast(lat: number, lon: number, altitude?: number, timeout = 10000): Promise<WeatherForecast> {
    const params = new URLSearchParams({
      lat: lat.toString(),
      lon: lon.toString(),
    });
    if (altitude) {
      params.append('altitude', altitude.toString());
    }
    
    await this.trackCardInteraction('weather', 'fetch_data');
    return this.request<WeatherForecast>(`/weather/forecast?${params}`, 'GET', undefined, timeout);
  }

  async getOsloWeather(timeout = 8000): Promise<WeatherForecast> {
    await this.trackCardInteraction('weather', 'fetch_oslo');
    return this.request<WeatherForecast>('/weather/oslo', 'GET', undefined, timeout);
  }

  // News API methods
  async getLatestNews(category?: string, limit = 5, offset = 0, maxAge?: number, timeout = 8000): Promise<NewsResponse> {
    const params = new URLSearchParams();
    if (category) params.append('category', category);
    if (limit) params.append('limit', limit.toString());
    if (offset) params.append('offset', offset.toString());
    if (maxAge) params.append('maxAge', maxAge.toString());
    
    const queryString = params.toString();
    await this.trackCardInteraction('news', 'fetch_data');
    return this.request<NewsResponse>(`/news${queryString ? `?${queryString}` : ''}`, 'GET', undefined, timeout);
  }

  async reverseGeocode(lat: number, lon: number): Promise<string> {
    const params = new URLSearchParams({ lat: lat.toString(), lon: lon.toString() });
    const res = await this.request<{ name: string }>(`/weather/reverse-geocode?${params.toString()}`);
    return res.name;
  }

  async getNewsCategories(): Promise<string[]> {
    return this.request<string[]>('/news/categories');
  }

  async getNewsSources(): Promise<Array<{ id: string; name: string; category?: string }>> {
    return this.request<Array<{ id: string; name: string; category?: string }>>('/news/sources');
  }

  // Aquatiq API methods
  async getAquatiqAds(category?: string, limit?: number): Promise<AquatiqResponse> {
    const params = new URLSearchParams();
    if (category) params.append('category', category);
    if (limit) params.append('limit', limit.toString());
    
    const queryString = params.toString();
    await this.trackCardInteraction('aquatiq', 'fetch_ads');
    return this.request<AquatiqResponse>(`/aquatiq/ads${queryString ? `?${queryString}` : ''}`);
  }

  async getAquatiqAd(id: string): Promise<AquatiqAd | null> {
    await this.trackCardInteraction('aquatiq', 'view_ad');
    return this.request<AquatiqAd | null>(`/aquatiq/ads/${id}`);
  }

  async getAquatiqCategories(): Promise<string[]> {
    return this.request<string[]>('/aquatiq/categories');
  }

  // Traffic API methods
  async getTrafficData(lat?: number, lon?: number, radius?: number, search?: string): Promise<TrafficResponse> {
    const params = new URLSearchParams();
    if (lat) params.append('lat', lat.toString());
    if (lon) params.append('lon', lon.toString());
    if (radius) params.append('radius', radius.toString());
    if (search) params.append('search', search);
    
    const queryString = params.toString();
    await this.trackCardInteraction('traffic', 'fetch_data');
    return this.request<TrafficResponse>(`/traffic${queryString ? `?${queryString}` : ''}`);
  }

  async searchTrafficData(query: string): Promise<TrafficResponse> {
    await this.trackCardInteraction('traffic', 'search');
    return this.request<TrafficResponse>(`/traffic/search?q=${encodeURIComponent(query)}`);
  }

  // Analytics API methods
  async getAnalyticsMetrics(timeRange: 'hour' | 'day' | 'week' | 'month' = 'day') {
    return this.request(`/analytics/metrics?timeRange=${timeRange}`);
  }

  async getUserActivity(userId: string, timeRange: 'day' | 'week' | 'month' = 'week') {
    return this.request(`/analytics/user/${userId}/activity?timeRange=${timeRange}`);
  }

  // Notification API methods
  async subscribeToPushNotifications(subscription: PushSubscription) {
    if (!this.userId) throw new Error('User ID required for notifications');
    return this.request(`/notifications/subscribe/${this.userId}`, 'POST', subscription);
  }

  async updateNotificationPreferences(preferences: Record<string, unknown>) {
    if (!this.userId) throw new Error('User ID required for notifications');
    return this.request(`/notifications/preferences/${this.userId}`, 'POST', preferences);
  }

  async getNotificationPreferences() {
    if (!this.userId) throw new Error('User ID required for notifications');
    return this.request(`/notifications/preferences/${this.userId}`);
  }

  async sendTestNotification() {
    if (!this.userId) throw new Error('User ID required for notifications');
    return this.request(`/notifications/test/${this.userId}`, 'POST');
  }

  // Request notification permission
  async requestNotificationPermission(): Promise<boolean> {
    if (!('Notification' in window)) {
      console.log('This browser does not support notifications');
      return false;
    }

    if (Notification.permission === 'granted') {
      return true;
    }

    if (Notification.permission !== 'denied') {
      const permission = await Notification.requestPermission();
      return permission === 'granted';
    }

    return false;
  }
}

export const apiClient = new ApiClient();
