'use client';

import { useEffect } from 'react';
import { useQuery, useMutation } from 'convex/react';
import { api } from '@/lib/convex-api-stub';

export interface UseLivePresenceOptions {
    userId?: string;
    conversationId?: string;
}

/**
 * useLivePresence
 * 
 * Convex hook to manage and subscribe to real-time presence (who is online, typing, etc).
 * This allows the UI to instantly show user avatars glowing when they are active
 * in the same conversation or workspace.
 */
export function useLivePresence(options: UseLivePresenceOptions = {}) {
    // Get internal Convex user reference
    const convexUser = useQuery(
        api.users.getByExternalAuthId,
        options.userId ? { externalAuthId: options.userId } : "skip"
    );

    // Subscribe to presence for a specific context (like a conversation)
    const activeUsers = useQuery(
        api.presence?.listByConversation || ("skip" as any), // Example stub matching schema
        options.conversationId ? { conversationId: options.conversationId } : "skip"
    );

    const updateLastSeen = useMutation((api as any).users.updateLastSeen);

    // Example: Ping presence every 30 seconds
    useEffect(() => {
        if (!convexUser) return;

        // Initial ping
        updateLastSeen({ userId: convexUser._id }).catch(console.error);

        // Set interval ping
        const interval = setInterval(() => {
            updateLastSeen({ userId: convexUser._id }).catch(console.error);
        }, 30000);

        return () => clearInterval(interval);
    }, [convexUser, updateLastSeen]);

    return {
        activeUsers: activeUsers || [],
        isTracking: !!convexUser,
    };
}
