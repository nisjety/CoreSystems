/**
 * AI Actions
 * 
 * Server-side functions that call external services (AI Core).
 * These run outside the transaction and can make HTTP requests.
 */

import { action } from "./_generated/server";
import { v } from "convex/values";
import { api } from "./_generated/api";

const CONVEX_INTERNAL_SERVICE_KEY =
  process.env.CONVEX_INTERNAL_SERVICE_KEY ||
  process.env.INTERNAL_API_KEY ||
  "change-me-internal-service-secret";

/**
 * Generate AI response for a user message
 * 
 * Flow:
 * 1. User sends message (already stored in DB)
 * 2. This action is triggered
 * 3. Calls AI Core with conversation history
 * 4. AI Core calls Org Core for RAG context
 * 5. Streams response chunks back
 * 6. Stores assistant response in DB
 */
export const generateResponse = action({
  args: {
    conversationId: v.id("conversations"),
    userMessageId: v.id("messages"),
  },
  handler: async (ctx, args) => {
    // Get conversation and messages
    const conversation = await ctx.runQuery(api.conversations.getById, {
      conversationId: args.conversationId,
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
    });
    
    if (!conversation) {
      throw new Error("Conversation not found");
    }
    
    const messages = await ctx.runQuery(api.messages.get, {
      conversationId: args.conversationId,
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
    });
    
    // Get org settings for AI Core
    const org = await ctx.runQuery(api.organizations.getById, {
      orgId: conversation.orgId,
      serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
    });
    
    // Prepare request for AI Core
    const aiCoreUrl = process.env.AI_CORE_URL!;
    const apiKey = process.env.AI_CORE_API_KEY!;
    
    try {
      // Create placeholder assistant message for streaming
      const assistantMessageId = await ctx.runMutation(api.messages.createStreaming, {
        conversationId: args.conversationId,
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
      });
      
      // Call AI Core streaming endpoint
      const response = await fetch(`${aiCoreUrl}/stream/chat`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Authorization": `Bearer ${apiKey}`,
          "X-Org-ID": conversation.orgId,
        },
        body: JSON.stringify({
          messages: messages.map(m => ({
            role: m.role,
            content: m.content,
          })),
          model: conversation.metadata?.model || org?.settings?.defaultModel || "gpt-4o-mini",
          temperature: conversation.metadata?.temperature || 0.7,
          max_tokens: conversation.metadata?.maxTokens || org?.settings?.maxTokens,
          org_id: conversation.orgId,
          session_id: conversation.sessionId,
        }),
      });
      
      if (!response.ok) {
        throw new Error(`AI Core error: ${response.statusText}`);
      }
      
      // Stream response chunks
      const reader = response.body!.getReader();
      const decoder = new TextDecoder();
      let fullContent = "";
      
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        
        const text = decoder.decode(value);
        const lines = text.split("\n");
        
        for (const line of lines) {
          if (line.startsWith("data: ")) {
            const data = JSON.parse(line.slice(6));
            
            if (data.type === "content") {
              fullContent += data.content;
              
              // Update streaming message
              await ctx.runMutation(api.messages.updateStreaming, {
                messageId: assistantMessageId,
                chunk: data.content,
                serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
              });
            }
            else if (data.type === "done") {
              // Finalize message
              await ctx.runMutation(api.messages.finalizeStreaming, {
                messageId: assistantMessageId,
                content: fullContent,
                serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
                metadata: {
                  model: data.metadata?.model,
                  tokens: data.metadata?.tokens,
                  latencyMs: data.metadata?.latency_ms,
                },
              });
            }
            else if (data.type === "error") {
              throw new Error(data.error);
            }
          }
        }
      }
      
      return { 
        success: true, 
        messageId: assistantMessageId,
        content: fullContent,
      };
      
    } catch (error) {
      // Log error and create error message
      console.error("AI generation failed:", error);
      
      const errorMessage = await ctx.runMutation(api.messages.create, {
        conversationId: args.conversationId,
        role: "assistant",
        content: "Sorry, I encountered an error generating a response. Please try again.",
        serviceKey: CONVEX_INTERNAL_SERVICE_KEY,
        metadata: {
          error: error instanceof Error ? error.message : "Unknown error",
        },
      });
      
      return {
        success: false,
        error: error instanceof Error ? error.message : "Unknown error",
        messageId: errorMessage,
      };
    }
  },
});

/**
 * Call AI Core for a specific query with RAG context
 */
export const queryWithContext = action({
  args: {
    orgId: v.string(),
    query: v.string(),
    conversationId: v.optional(v.id("conversations")),
  },
  handler: async (ctx, args) => {
    const aiCoreUrl = process.env.AI_CORE_URL!;
    const apiKey = process.env.AI_CORE_API_KEY!;
    
    const response = await fetch(`${aiCoreUrl}/chat`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${apiKey}`,
        "X-Org-ID": String(args.orgId),
      },
      body: JSON.stringify({
        query: args.query,
        use_rag: true,
        conversation_id: args.conversationId,
      }),
    });
    
    if (!response.ok) {
      throw new Error(`AI Core error: ${response.statusText}`);
    }
    
    return await response.json();
  },
});
