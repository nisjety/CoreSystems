import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { apiClient } from '@/components/chat/lib/api';

// Weather hooks
export const useWeatherForecast = (lat?: number, lon?: number, enabled = true) => {
  return useQuery({
    queryKey: ['weather', 'forecast', lat, lon],
    queryFn: () => {
      if (!lat || !lon) throw new Error('Coordinates required');
      return apiClient.getWeatherForecast(lat, lon);
    },
    enabled: enabled && !!lat && !!lon,
    staleTime: 10 * 60 * 1000, // 10 minutes
    retry: 2,
  });
};

export const useOsloWeather = () => {
  return useQuery({
    queryKey: ['weather', 'oslo'],
    queryFn: () => apiClient.getOsloWeather(),
    staleTime: 10 * 60 * 1000, // 10 minutes
    retry: 2,
  });
};

// News hooks
export const useLatestNews = (
  category?: string, 
  limit = 5, 
  offset = 0, 
  maxAge?: number
) => {
  return useQuery({
    queryKey: ['news', 'latest', category, limit, offset, maxAge],
    queryFn: () => apiClient.getLatestNews(category, limit, offset, maxAge),
    staleTime: 5 * 60 * 1000, // 5 minutes
    retry: 2,
  });
};

export const useNewsCategories = () => {
  return useQuery({
    queryKey: ['news', 'categories'],
    queryFn: () => apiClient.getNewsCategories(),
    staleTime: 60 * 60 * 1000, // 1 hour
  });
};

// Aquatiq hooks
export const useAquatiqAds = (category?: string, limit?: number) => {
  return useQuery({
    queryKey: ['aquatiq', 'ads', category, limit],
    queryFn: () => apiClient.getAquatiqAds(category, limit),
    staleTime: 15 * 60 * 1000, // 15 minutes
    retry: 2,
  });
};

export const useAquatiqCategories = () => {
  return useQuery({
    queryKey: ['aquatiq', 'categories'],
    queryFn: () => apiClient.getAquatiqCategories(),
    staleTime: 60 * 60 * 1000, // 1 hour
  });
};

// Traffic hooks
export const useTrafficData = (
  lat?: number,
  lon?: number,
  radius: number = 100,
  search?: string
) => {
  return useQuery({
    queryKey: ['traffic', lat, lon, radius, search],
    queryFn: async () => {
      const response = await apiClient.getTrafficData(lat, lon, radius);
      
      if (!response.success) {
        throw new Error(response.error || 'Failed to fetch traffic data');
      }

      // Remove duplicates and filter by search if provided
      const seen = new Set<string>();
      let cleaned = response.data.filter(p => {
        if (seen.has(p.id)) return false;
        seen.add(p.id);
        return true;
      });

      if (search && search.trim()) {
        const normalize = (s: string) => s
          .toLowerCase()
          .normalize('NFD')
          .replace(/\p{Diacritic}/gu, '')
          .replace(/[^a-z0-9\s]/g, '');
        
        const q = normalize(search);
        cleaned = cleaned.filter(p => {
          const nameN = normalize(p.name || '');
          const locN = normalize(p.location || '');
          return nameN.includes(q) || locN.includes(q);
        });
      }

      return {
        data: cleaned,
        count: cleaned.length,
        lastUpdated: new Date()
      };
    },
    enabled: !!lat && !!lon, // Only run query when we have coordinates
    staleTime: 5 * 60 * 1000, // 5 minutes
    refetchInterval: 10 * 60 * 1000, // Refetch every 10 minutes
  });
};

export const useSearchTrafficData = () => {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: (query: string) => apiClient.searchTrafficData(query),
    onSuccess: (data) => {
      // Update the cache with the search results
      queryClient.setQueryData(['traffic', 'search', data], data);
    },
  });
};

// Utility hooks
export const useReverseGeocode = (lat?: number, lon?: number) => {
  return useQuery({
    queryKey: ['geocode', 'reverse', lat, lon],
    queryFn: () => {
      if (!lat || !lon) throw new Error('Coordinates required');
      return apiClient.reverseGeocode(lat, lon);
    },
    enabled: !!lat && !!lon,
    staleTime: 60 * 60 * 1000, // 1 hour
  });
};

// Analytics hooks
export const useTrackEvent = () => {
  return useMutation({
    mutationFn: (event: Parameters<typeof apiClient.trackEvent>[0]) => 
      apiClient.trackEvent(event),
  });
};

export const useTrackCardInteraction = () => {
  return useMutation({
    mutationFn: ({ cardType, action }: { cardType: string; action: string }) =>
      apiClient.trackCardInteraction(cardType, action),
    onError: (error) => {
      console.error('Error tracking card interaction:', error);
    },
  });
};
