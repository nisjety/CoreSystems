# AI-Powered Session Title Generation

## Overview

The AI Title Generation feature automatically creates intelligent, descriptive titles for chat sessions based on the conversation content. This feature enhances user experience by providing meaningful session names that make it easy to identify and navigate between different conversations.

## Features

### 🤖 Intelligent Title Generation
- Uses GPT-4o-mini model for cost-effective, high-quality title generation
- Analyzes conversation context including user messages and assistant responses
- Generates Norwegian titles (max 30 characters) that summarize the main topic
- Falls back to keyword-based generation if AI service is unavailable

### 🔄 Automatic Title Updates
- Automatically generates titles after 3+ messages in a session
- Only updates sessions with default titles ("Ny samtale" or "Samtale [ID]")
- Preserves manually set titles to respect user preferences
- Runs asynchronously to avoid blocking chat flow

### 🎯 Smart Content Analysis
- **Keyword Extraction**: Identifies important terms and concepts
- **Question Detection**: Recognizes question-based conversations
- **Technical Term Recognition**: Handles technical discussions appropriately
- **Language Detection**: Optimizes for Norwegian and English content
- **Context Preservation**: Uses recent message history for accurate titles

### 📋 Multiple Access Methods
1. **Automatic**: Triggered after sufficient conversation context
2. **Manual Command**: `/ai-tittel <session-id>` for manual generation
3. **Adaptive Card Action**: 🤖 button in Thread Switcher interface
4. **Fallback Generation**: Keyword-based titles when AI is unavailable

## Usage Examples

### Automatic Title Generation
```
User: "Kan du hjelpe meg med å lage en Docker-container for min Next.js app?"
Assistant: "Selvfølgelig! Jeg kan hjelpe deg..."
[After 3+ messages, title becomes: "🤖 Docker Next.js Setup"]
```

### Manual Command Usage
```
/ai-tittel abc12345
→ "🤖 AI genererte ny tittel: 'Database Migration Guide'"
```

### Adaptive Card Integration
- Thread Switcher shows 🤖 icon for AI-generated titles
- Click 🤖 button to regenerate title based on current conversation content
- Visual distinction between manual and AI-generated titles

## Technical Implementation

### Service Architecture
```typescript
AiTitleGeneratorService
├── generateTitle() - Main entry point for title generation
├── generateWithAI() - Uses AI Orchestrator for smart titles
├── generateFromKeywords() - Fallback keyword-based generation
├── prepareConversationContext() - Analyzes message content
└── sanitizeTitle() - Ensures valid, clean titles
```

### AI Integration
- **Model**: GPT-4o-mini (cost-effective for title generation)
- **Endpoint**: AI Orchestrator `/api/ai/chat/completions`
- **Context**: System prompt optimized for Norwegian titles
- **Retry Logic**: 3 attempts with exponential backoff
- **Timeout**: 10 seconds per request

### Content Analysis Features
```typescript
ConversationContext {
  messageCount: number;           // Total messages analyzed
  userMessageCount: number;       // User-specific message count
  firstUserMessage: string;       // Initial user input
  lastUserMessage: string;        // Most recent user input
  allText: string;               // Combined conversation text
  hasQuestions: boolean;         // Contains question words/marks
  hasTechnicalTerms: boolean;    // Contains technical vocabulary
  language: string;              // Detected language (no/en)
  keywords: string[];            // Extracted meaningful terms
}
```

### Error Handling & Fallbacks
1. **AI Service Unavailable**: Falls back to keyword extraction
2. **Invalid Responses**: Uses first meaningful message words
3. **Empty Content**: Generates "Samtale [ID]" format
4. **Network Timeouts**: Retries with exponential backoff
5. **Rate Limiting**: Graceful degradation to simple generation

## Configuration

### Environment Variables
```bash
AI_ORCHESTRATOR_URL=http://localhost:3001  # AI service endpoint
```

### Title Generation Settings
- **Max Length**: 30 characters (truncated with "...")
- **Min Length**: 3 characters (fallback to "Samtale")
- **AI Model**: gpt-4o-mini
- **Temperature**: 0.3 (focused, consistent outputs)
- **Max Tokens**: 50 (sufficient for titles)

### Stop Words (Filtered Out)
- **Norwegian**: og, i, å, for, på, med, av, til, er, som, ikke...
- **English**: the, and, is, in, to, of, a, that, it, with...

## User Experience

### Visual Indicators
- **🤖 Prefix**: Shows AI-generated titles in Thread Switcher
- **Status Messages**: Clear feedback during generation process
- **Error Messages**: User-friendly Norwegian error descriptions

### Command Help Integration
```
/ai-tittel <id> - Generer AI-tittel basert på innhold
🤖 AI-tittel: Kommandoen analyserer samtaleinnholdet og lager en smart tittel.
```

### Adaptive Card Actions
- **Generate AI Title**: 🤖 button for manual regeneration
- **Loading State**: Shows progress during generation
- **Success/Error Cards**: Clear feedback with generated titles

## Performance Considerations

### Optimization Strategies
- **Lazy Loading**: Only generates when needed (3+ messages)
- **Async Processing**: Non-blocking title generation
- **Content Limits**: Analyzes last 10 messages for performance
- **Caching**: Avoids regenerating existing good titles
- **Rate Limiting**: Respects AI service limits

### Cost Management
- **Efficient Model**: Uses gpt-4o-mini for cost-effectiveness
- **Token Optimization**: Minimal prompt and response tokens
- **Fallback Strategy**: Reduces AI calls when possible
- **Smart Triggers**: Only generates for sessions needing titles

## Testing & Validation

### Test Scenarios
1. **New Session**: Auto-generation after 3 messages
2. **Manual Command**: `/ai-tittel` with valid session ID
3. **Adaptive Card**: Button click generates new title
4. **Error Handling**: Service unavailable, invalid session
5. **Fallback**: Keyword-based generation when AI fails

### Quality Validation
- **Title Length**: Must be 3-30 characters
- **Character Safety**: Removes invalid characters
- **Language Consistency**: Norwegian titles for Norwegian content
- **Meaningful Content**: Avoids generic or unhelpful titles

## Future Enhancements

### Planned Improvements
- **Multi-language Support**: Better detection and generation
- **User Preferences**: Customizable auto-generation settings
- **Template System**: User-defined title templates
- **Conversation Summaries**: Longer descriptions for complex topics
- **Learning System**: Improve based on user title edits

### Integration Opportunities
- **Search Enhancement**: Better searchable titles improve findability
- **Analytics**: Track title generation success rates
- **User Feedback**: Allow rating of generated titles
- **Export Features**: Include smart titles in conversation exports

## Security & Privacy

### Data Handling
- **Content Analysis**: Only recent messages analyzed (max 10)
- **No Persistence**: Conversation content not stored in AI service
- **User Context**: Includes user ID for rate limiting only
- **GDPR Compliance**: Works with existing session data handling

### Service Integration
- **Internal Only**: Uses internal AI Orchestrator service
- **No External APIs**: All processing within infrastructure
- **Error Isolation**: AI failures don't break core functionality
- **Audit Trail**: Logs generation attempts and outcomes

This AI-powered title generation feature significantly improves the user experience by providing meaningful, searchable conversation titles while maintaining performance and cost-effectiveness through intelligent fallback strategies.
