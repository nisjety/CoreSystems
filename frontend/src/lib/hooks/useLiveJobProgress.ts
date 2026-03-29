'use client';

import { useQuery } from 'convex/react';
import { api } from '@/lib/convex-api-stub';

export interface UseLiveJobProgressOptions {
    userId?: string;
    orgId?: string;
    limit?: number;
}

/**
 * useLiveJobProgress
 * 
 * Convex hook to subscribe to the real-time status of backend ingestion/scraping jobs.
 * This directly queries the Application Plane, which reflects state changes published
 * over NATS by Data Plane, Ingestion Plane, etc.
 */
export function useLiveJobProgress(options: UseLiveJobProgressOptions = {}) {
    // Query jobs by the user's ID
    // Make sure to resolve the external auth string into a Convex internal ID
    const convexUser = useQuery(
        api.users.getByExternalAuthId,
        options.userId ? { externalAuthId: options.userId } : "skip"
    );

    const jobs = useQuery(
        api.jobs?.listByUser || ("skip" as any), // Example stub if you add listByUser
        convexUser ? { userId: convexUser._id, limit: options.limit || 5 } : "skip"
    );

    return {
        jobs: jobs || [],
        isLoading: convexUser === undefined || jobs === undefined,
        orgId: convexUser?.orgId,
    };
}
