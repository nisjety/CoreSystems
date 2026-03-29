/**
 * Note Server Service Client
 * 
 * Type-safe client for note-server providing meetings, transcripts, and semantic search
 */

// Types from note-server API
export interface Meeting {
  id: string;
  title: string;
  start_time: string;
  end_time?: string;
  created_at: string;
  updated_at: string;
  deleted_at?: string;
}

export interface Transcript {
  id: string;
  meeting_id: string;
  full_text: string;
  language: string;
  segments?: TranscriptSegment[];
  word_count?: number;
  speakers?: Speaker[];
  source?: string;
  transcription_model?: string;
  confidence_score?: number;
  created_at: string;
  updated_at: string;
}

export interface TranscriptSegment {
  start: number;
  end: number;
  text: string;
  speaker?: string;
  confidence?: number;
}

export interface Speaker {
  id: string;
  name?: string;
  segments: number[];
}

export interface SemanticSearchResult {
  id: string;
  meeting_id: string;
  score: number;
  snippet: string;
}

export interface HistoryItem {
  id: string;
  meeting_id: string;
  title: string;
  timestamp: string;
  event_type: 'meeting_created' | 'transcript_uploaded' | 'note_taken';
}

class NoteServerServiceAPI {
  private baseURL: string;
  private token: string | null = null;

  constructor() {
    // Use Next.js API proxy route
    this.baseURL = '/api/notes';
    
    // Load token from localStorage if available
    if (typeof window !== 'undefined') {
      this.token = localStorage.getItem('authToken');
    }
  }

  private async makeRequest<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    try {
      const headers: Record<string, string> = {
        'Content-Type': 'application/json',
        ...options.headers as Record<string, string>,
      };

      // Add JWT token if available
      if (this.token) {
        headers['Authorization'] = `Bearer ${this.token}`;
      }

      const response = await fetch(`${this.baseURL}${endpoint}`, {
        ...options,
        headers,
        credentials: 'include',
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.message || `HTTP ${response.status}: ${response.statusText}`);
      }

      const data = await response.json();
      return data as T;
    } catch (error) {
      console.error(`Note Server Error [${endpoint}]:`, error);
      throw error;
    }
  }

  setToken(token: string | null) {
    this.token = token;
    if (typeof window !== 'undefined' && token) {
      localStorage.setItem('authToken', token);
    }
  }

  // Meeting endpoints
  async getMeetings(limit = 50, offset = 0): Promise<Meeting[]> {
    return this.makeRequest<Meeting[]>(`/meetings?limit=${limit}&offset=${offset}`);
  }

  async getMeeting(id: string): Promise<Meeting> {
    return this.makeRequest<Meeting>(`/meetings/${id}`);
  }

  async createMeeting(data: { title: string; start_time: string; end_time?: string }): Promise<Meeting> {
    return this.makeRequest<Meeting>('/meetings', {
      method: 'POST',
      body: JSON.stringify(data),
    });
  }

  // Transcript endpoints
  async getTranscript(meetingId: string): Promise<Transcript | null> {
    try {
      return await this.makeRequest<Transcript>(`/meetings/${meetingId}/transcript`);
    } catch (error) {
      // Return null if no transcript exists
      return null;
    }
  }

  async uploadTranscript(meetingId: string, text: string, language = 'en'): Promise<Transcript> {
    return this.makeRequest<Transcript>(`/meetings/${meetingId}/transcript`, {
      method: 'POST',
      body: JSON.stringify({ text, language }),
    });
  }

  // Semantic search
  async semanticSearch(query: string, topK = 10): Promise<SemanticSearchResult[]> {
    const params = new URLSearchParams({
      q: query,
      topK: topK.toString(),
    });
    return this.makeRequest<SemanticSearchResult[]>(`/search/semantic?${params}`);
  }

  // History endpoints
  async getYesterdayHistory(): Promise<HistoryItem[]> {
    return this.makeRequest<HistoryItem[]>('/history/yesterday');
  }

  async getThisWeekHistory(): Promise<HistoryItem[]> {
    return this.makeRequest<HistoryItem[]>('/history/this-week');
  }

  async getThisMonthHistory(): Promise<HistoryItem[]> {
    return this.makeRequest<HistoryItem[]>('/history/this-month');
  }
}

// Export singleton instance
export const noteServerService = new NoteServerServiceAPI();
