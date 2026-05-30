# OpenClaw Architectural Patterns - Implementation Guide

## Summary of Implemented Improvements (CoreSystem)

### ✅ 1. Event Hooks System
**File**: `/backend/Org-core/internal/rag/events.go`

Implemented event-driven architecture for RAG operations:
- `EventIndexStart` - Fired when document indexing begins
- `EventIndexComplete` - Fired after indexing completes (with success/failure metrics)
- `EventQueryStart` - Fired when retrieval query begins
- `EventQueryComplete` - Fired after query completes (with results/latency)
- `EventDeleteStart/Complete` - Document deletion lifecycle

**Usage Pattern**:
```go
// Register event handlers
service.eventEmitter.On(EventIndexComplete, func(ctx context.Context, event RAGEvent) error {
    log.Info().
        Str("org_id", event.OrgID).
        Int("success", event.Metadata["success"].(int)).
        Msg("Documents indexed")
    return nil
})

// Events fire asynchronously
s.eventEmitter.EmitAsync(ctx, RAGEvent{
    Type: EventIndexStart,
    OrgID: orgID,
    Metadata: map[string]interface{}{"doc_count": 10},
})
```

### ✅ 2. Duplicate Detection
**File**: `/backend/Org-core/internal/rag/service_impl.go`

Added similarity-based duplicate detection (0.95 threshold) before indexing:
```go
// Check if similar document already exists
searchResults, err := s.vectorStore.Search(ctx, doc.OrgID, denseEmbeddings[0], 1, nil)
if err == nil && len(searchResults) > 0 && searchResults[0].Score > 0.95 {
    return fmt.Errorf("duplicate document detected (similarity: %.2f)", searchResults[0].Score)
}
```

**Benefits**:
- Prevents redundant indexing
- Saves embedding API costs
- Maintains data quality

### ✅ 3. Document Importance & Category
**Files**: 
- `/backend/Org-core/migrations/008_rag_importance.up.sql`
- `/backend/Org-core/migrations/008_rag_importance.down.sql`

Added metadata fields to `rag_documents`:
```sql
ALTER TABLE rag_documents ADD COLUMN importance REAL DEFAULT 0.5;
ALTER TABLE rag_documents ADD COLUMN category TEXT DEFAULT 'general';
CREATE INDEX idx_rag_documents_importance ON rag_documents(importance DESC);
```

**Use Cases**:
- **Importance** (0-1): Prioritize critical documents in retrieval
- **Category**: Filter searches (`technical`, `business`, `legal`, `general`)

### ✅ 4. Tool-Based API for AI Agents
**File**: `/backend/Org-core/internal/http/rag_tools.go`

Created OpenAI Function Calling compatible schema:
- `GET /api/v1/rag/tools/schema` - Returns JSON schemas for AI tools

**Endpoint**: `http://localhost:8080/api/v1/rag/tools/schema`

**Tools Exposed**:
1. **memory_recall** - Search knowledge base
2. **memory_store** - Index new documents
3. **memory_forget** - Delete documents (GDPR-compliant)
4. **memory_agentic** - Advanced retrieval with reasoning

---

## OpenClaw Channel Architecture Patterns

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Gateway (Control Plane)                │
│                    ws://127.0.0.1:18789                     │
└──────────────────┬──────────────────────────────────────────┘
                   │
       ┌───────────┼───────────┐
       │           │           │
    ┌──▼──┐     ┌─▼──┐     ┌─▼───┐
    │ WA  │     │ TG │     │ Slack│
    │Plugin│     │Plugin│    │Plugin│
    └─────┘     └────┘     └──────┘
       │           │           │
    ┌──▼───────────▼───────────▼────┐
    │    Messaging Channel API      │
    │  (Baileys, grammY, Bolt)      │
    └───────────────────────────────┘
```

### Key Design Patterns from OpenClaw

#### 1. **Plugin-Based Channel System**
Each channel is a **self-contained plugin** with:
- **Manifest** (`openclaw.plugin.json` or `package.json` with `channel` key)
- **Catalog Entry** - Metadata for UI/discovery
- **Installation Info** - npm spec or local path

**Channel Manifest Structure**:
```typescript
{
  "channel": {
    "id": "whatsapp",
    "label": "WhatsApp",
    "selectionLabel": "WhatsApp (Baileys)",
    "detailLabel": "WhatsApp via Baileys",
    "docsPath": "/channels/whatsapp",
    "systemImage": "message.fill",
    "order": 10,
    "showConfigured": true,
    "quickstartAllowFrom": true,
    "forceAccountBinding": false
  },
  "install": {
    "npmSpec": "@openclaw/whatsapp",
    "localPath": "./channels/whatsapp",
    "defaultChoice": "npm"
  }
}
```

#### 2. **Channel Configuration Pattern**
Centralized config with **environment variable precedence**:
```typescript
// config-schema.ts pattern
export const whatsappConfigSchema = {
  enabled: boolean,
  allowFrom: string[],  // Allowlist for DMs
  groups: {             // Group-specific configs
    "*": {
      requireMention: boolean,
      allowFrom: string[]
    }
  },
  mediaMaxMb: number
};
```

**Environment Variable Override**:
```bash
OPENCLAW_WHATSAPP_ENABLED=true
OPENCLAW_WHATSAPP_ALLOW_FROM="user1,user2"
```

#### 3. **Allowlist & Security Pattern**
**DM Pairing by Default**:
- Unknown senders get **pairing code**
- Must be approved via: `openclaw pairing approve whatsapp <code>`
- Adds sender to local allowlist store

**Group Mention Gating**:
```typescript
// Only respond to @mentions in groups
{
  groups: {
    "*": {
      requireMention: true
    }
  }
}
```

#### 4. **Message Normalization**
All channels output **normalized message format**:
```typescript
interface NormalizedMessage {
  id: string;
  chatId: string;
  senderId: string;
  text?: string;
  media?: {
    type: 'image' | 'video' | 'audio' | 'document';
    url?: string;
    buffer?: Buffer;
    caption?: string;
  };
  replyTo?: string;
  timestamp: number;
  raw: any;  // Original channel-specific message
}
```

#### 5. **Outbound Message Actions**
Standardized actions across channels:
```typescript
interface ChannelActions {
  sendText(chatId: string, text: string, options?: SendOptions): Promise<void>;
  sendMedia(chatId: string, media: Media, options?: SendOptions): Promise<void>;
  react(chatId: string, messageId: string, emoji: string): Promise<void>;
  delete(chatId: string, messageId: string): Promise<void>;
  typing(chatId: string, isTyping: boolean): Promise<void>;
}
```

#### 6. **Registry Pattern**
```typescript
// registry.ts
export class ChannelRegistry {
  private channels = new Map<string, Channel>();
  
  register(id: string, channel: Channel) {
    this.channels.set(id, channel);
  }
  
  get(id: string): Channel | undefined {
    return this.channels.get(id);
  }
  
  list(): string[] {
    return Array.from(this.channels.keys());
  }
}
```

---

## Go Implementation Strategy for CoreSystem

### Recommended Architecture

```
backend/
  channels/
    registry/
      registry.go          # Channel registry pattern
    types/
      types.go             # Shared interfaces
      message.go           # Normalized message format
    plugins/
      whatsapp/
        plugin.go          # WhatsApp implementation
        config.go          # WhatsApp config
      telegram/
        plugin.go
        config.go
      slack/
        plugin.go
        config.go
      discord/
        plugin.go
        config.go
    server/
      server.go            # Channel gateway server
```

### Core Interfaces

```go
// channels/types/types.go
package types

import (
    "context"
    "time"
)

// Channel represents a messaging channel plugin
type Channel interface {
    // Metadata
    ID() string
    Label() string
    
    // Lifecycle
    Start(ctx context.Context) error
    Stop(ctx context.Context) error
    HealthCheck(ctx context.Context) error
    
    // Inbound (receive messages)
    OnMessage(handler MessageHandler)
    
    // Outbound (send messages)
    SendText(ctx context.Context, chatID, text string, opts *SendOptions) error
    SendMedia(ctx context.Context, chatID string, media *Media, opts *SendOptions) error
    React(ctx context.Context, chatID, messageID, emoji string) error
    SetTyping(ctx context.Context, chatID string, isTyping bool) error
}

// NormalizedMessage is the standard format for all channels
type NormalizedMessage struct {
    ID        string
    ChannelID string  // "whatsapp", "telegram", etc.
    ChatID    string
    SenderID  string
    Text      *string
    Media     *Media
    ReplyTo   *string
    Timestamp time.Time
    Raw       interface{} // Channel-specific raw message
}

// MessageHandler processes incoming messages
type MessageHandler func(ctx context.Context, msg *NormalizedMessage) error

// Media represents media attachments
type Media struct {
    Type    MediaType
    URL     *string
    Buffer  []byte
    Caption *string
}

type MediaType string

const (
    MediaTypeImage    MediaType = "image"
    MediaTypeVideo    MediaType = "video"
    MediaTypeAudio    MediaType = "audio"
    MediaTypeDocument MediaType = "document"
)

// SendOptions for customizing message delivery
type SendOptions struct {
    ReplyTo   *string
    ParseMode *string
    Silent    bool
}
```

### Example WhatsApp Plugin

```go
// channels/plugins/whatsapp/plugin.go
package whatsapp

import (
    "context"
    
    "github.com/triodelab/coresystem/channels/types"
)

type WhatsAppChannel struct {
    id          string
    config      *Config
    client      *WhatsAppClient // Wrapper around baileys or whatsmeow
    msgHandler  types.MessageHandler
}

func NewWhatsAppChannel(config *Config) *WhatsAppChannel {
    return &WhatsAppChannel{
        id:     "whatsapp",
        config: config,
    }
}

func (c *WhatsAppChannel) ID() string {
    return c.id
}

func (c *WhatsAppChannel) Label() string {
    return "WhatsApp"
}

func (c *WhatsAppChannel) Start(ctx context.Context) error {
    // Initialize WhatsApp client
    client, err := NewWhatsAppClient(c.config)
    if err != nil {
        return err
    }
    c.client = client
    
    // Start listening for messages
    go c.listen(ctx)
    
    return nil
}

func (c *WhatsAppChannel) OnMessage(handler types.MessageHandler) {
    c.msgHandler = handler
}

func (c *WhatsAppChannel) listen(ctx context.Context) {
    c.client.OnMessage(func(rawMsg *WhatsAppMessage) {
        // Normalize message
        normalized := &types.NormalizedMessage{
            ID:        rawMsg.ID,
            ChannelID: c.id,
            ChatID:    rawMsg.ChatID,
            SenderID:  rawMsg.From,
            Text:      &rawMsg.Text,
            Timestamp: rawMsg.Timestamp,
            Raw:       rawMsg,
        }
        
        // Apply allowlist filtering
        if !c.isAllowed(normalized.SenderID) {
            // Send pairing code
            return
        }
        
        // Call handler
        if c.msgHandler != nil {
            c.msgHandler(ctx, normalized)
        }
    })
}

func (c *WhatsAppChannel) SendText(ctx context.Context, chatID, text string, opts *types.SendOptions) error {
    return c.client.SendText(chatID, text)
}
```

### Channel Registry

```go
// channels/registry/registry.go
package registry

import (
    "fmt"
    "sync"
    
    "github.com/triodelab/coresystem/channels/types"
)

type Registry struct {
    mu       sync.RWMutex
    channels map[string]types.Channel
}

func NewRegistry() *Registry {
    return &Registry{
        channels: make(map[string]types.Channel),
    }
}

func (r *Registry) Register(channel types.Channel) error {
    r.mu.Lock()
    defer r.mu.Unlock()
    
    id := channel.ID()
    if _, exists := r.channels[id]; exists {
        return fmt.Errorf("channel %s already registered", id)
    }
    
    r.channels[id] = channel
    return nil
}

func (r *Registry) Get(id string) (types.Channel, error) {
    r.mu.RLock()
    defer r.mu.RUnlock()
    
    channel, exists := r.channels[id]
    if !exists {
        return nil, fmt.Errorf("channel %s not found", id)
    }
    
    return channel, nil
}

func (r *Registry) List() []string {
    r.mu.RLock()
    defer r.mu.RUnlock()
    
    ids := make([]string, 0, len(r.channels))
    for id := range r.channels {
        ids = append(ids, id)
    }
    return ids
}
```

---

## Go WhatsApp/Telegram Libraries

### Recommended Go Libraries

#### WhatsApp
- **whatsmeow** (https://github.com/tulir/whatsmeow)
  - Pure Go implementation
  - Multi-device support
  - Active development
  ```bash
  go get go.mau.fi/whatsmeow
  ```

#### Telegram
- **telegram-bot-api** (https://github.com/go-telegram-bot-api/telegram-bot-api)
  - Official Go wrapper
  - Simple API
  ```bash
  go get github.com/go-telegram-bot-api/telegram-bot-api/v5
  ```

#### Slack
- **slack-go** (https://github.com/slack-go/slack)
  - Official Slack SDK
  ```bash
  go get github.com/slack-go/slack
  ```

#### Discord
- **discordgo** (https://github.com/bwmarrin/discordgo)
  - Most popular Go Discord library
  ```bash
  go get github.com/bwmarrin/discordgo
  ```

---

## Next Steps for CoreSystem

1. **Run migration 008** to add importance/category fields
2. **Test new endpoints**:
   ```bash
   # Get AI agent tool schema
   curl http://localhost:8080/api/v1/rag/tools/schema | jq
   
   # Test with importance/category
   curl -X POST http://localhost:8080/api/v1/rag/documents \
     -H "X-Org-ID: 00000000-0000-0000-0000-000000000001" \
     -H "Content-Type: application/json" \
     -d '{
       "documents": [{
         "content": "Critical system documentation",
         "title": "Production Deployment Guide",
         "importance": 0.9,
         "category": "technical"
       }]
     }'
   ```

3. **Add event listeners**:
   ```go
   // In main.go or service initialization
   ragService.eventEmitter.On(rag.EventIndexComplete, func(ctx context.Context, event rag.RAGEvent) error {
       // Log to analytics, update UI, trigger workflows
       log.Info().Interface("metadata", event.Metadata).Msg("Indexing complete")
       return nil
   })
   ```

4. **Start channel implementation**:
   - Create `channels/` directory structure
   - Implement WhatsApp plugin using `whatsmeow`
   - Add channel registry to `cmd/server/main.go`

---

## References

- **OpenClaw GitHub**: https://github.com/openclaw/openclaw
- **OpenClaw Docs**: https://docs.openclaw.ai/
- **Channel Architecture**: https://docs.openclaw.ai/concepts/architecture
- **Plugin System**: https://docs.openclaw.ai/tools/skills
- **Memory Extensions**: https://github.com/openclaw/openclaw/tree/main/extensions/memory-lancedb
