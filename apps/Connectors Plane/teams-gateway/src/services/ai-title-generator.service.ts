import { Injectable, Logger } from '@nestjs/common';
import { HttpService } from '@nestjs/axios';
import { ConfigService } from '@nestjs/config';
import { firstValueFrom } from 'rxjs';
import { SessionMessage, MessageRole } from '../entities/session-message.entity';

export interface TitleGenerationRequest {
  messages: SessionMessage[];
  userId: string;
  sessionId: string;
}

export interface TitleGenerationResponse {
  title: string;
  confidence: number;
  fallbackUsed: boolean;
}

@Injectable()
export class AiTitleGeneratorService {
  private readonly logger = new Logger(AiTitleGeneratorService.name);
  private readonly aiOrchestratorUrl: string;
  private readonly maxRetries = 3;
  private readonly retryDelay = 1000;

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.aiOrchestratorUrl = this.configService.get<string>('AI_ORCHESTRATOR_URL') || 'http://localhost:3021';
  }

  /**
   * Generate intelligent title based on conversation content
   */
  async generateTitle(request: TitleGenerationRequest): Promise<TitleGenerationResponse> {
    try {
      // Prepare conversation context for AI
      const conversationContext = this.prepareConversationContext(request.messages);
      
      if (conversationContext.messageCount === 0) {
        return this.getFallbackTitle(request.sessionId);
      }

      // Try AI-powered generation first
      try {
        const aiTitle = await this.generateWithAI(conversationContext, request.userId);
        if (aiTitle && aiTitle.length > 0) {
          return {
            title: this.sanitizeTitle(aiTitle),
            confidence: 0.8,
            fallbackUsed: false
          };
        }
      } catch (error) {
        this.logger.warn(`AI title generation failed for session ${request.sessionId}: ${error.message}`);
      }

      // Fallback to smart keyword extraction
      const keywordTitle = this.generateFromKeywords(conversationContext);
      return {
        title: keywordTitle,
        confidence: 0.6,
        fallbackUsed: true
      };

    } catch (error) {
      this.logger.error(`Title generation failed for session ${request.sessionId}: ${error.message}`);
      return this.getFallbackTitle(request.sessionId);
    }
  }

  /**
   * Generate title using AI Orchestrator
   */
  private async generateWithAI(context: ConversationContext, userId: string): Promise<string> {
    const prompt = this.buildTitlePrompt(context);
    
    const requestData = {
      model: 'gpt-4o-mini', // Use cost-effective model for title generation
      messages: [
        {
          role: 'system',
          content: 'Du er en ekspert på å lage korte, beskrivende titler for samtaler. Lag en kort norsk tittel (maks 30 tegn) som oppsummerer hovedemnet i samtalen. Svar kun med tittelen, ingen annen tekst.'
        },
        {
          role: 'user',
          content: prompt
        }
      ],
      max_tokens: 50,
      temperature: 0.3,
      user: userId
    };

    let lastError: Error;
    for (let attempt = 1; attempt <= this.maxRetries; attempt++) {
      try {
        const response = await firstValueFrom(
          this.httpService.post(`${this.aiOrchestratorUrl}/api/ai/chat/completions`, requestData, {
            timeout: 10000,
            headers: {
              'Content-Type': 'application/json'
            }
          })
        );

        if (response.data?.choices?.[0]?.message?.content) {
          const title = response.data.choices[0].message.content.trim();
          this.logger.debug(`AI generated title (attempt ${attempt}): "${title}"`);
          return title;
        }

        throw new Error('Invalid response from AI Orchestrator');

      } catch (error) {
        lastError = error;
        this.logger.warn(`AI title generation attempt ${attempt} failed: ${error.message}`);
        
        if (attempt < this.maxRetries) {
          await this.delay(this.retryDelay * attempt);
        }
      }
    }

    throw lastError;
  }

  /**
   * Prepare conversation context for analysis
   */
  private prepareConversationContext(messages: SessionMessage[]): ConversationContext {
    // Get recent messages, prioritizing user messages
    const recentMessages = messages
      .slice(-10) // Last 10 messages
      .filter(msg => msg.content && msg.content.trim().length > 0);

    const userMessages = recentMessages.filter(msg => msg.role === MessageRole.USER);
    const assistantMessages = recentMessages.filter(msg => msg.role === MessageRole.ASSISTANT);

    // Extract key information
    const allText = recentMessages.map(msg => msg.content).join(' ');
    const firstUserMessage = userMessages[0]?.content || '';
    const lastUserMessage = userMessages[userMessages.length - 1]?.content || '';

    return {
      messageCount: recentMessages.length,
      userMessageCount: userMessages.length,
      firstUserMessage,
      lastUserMessage,
      allText,
      hasQuestions: this.containsQuestions(allText),
      hasTechnicalTerms: this.containsTechnicalTerms(allText),
      language: this.detectLanguage(allText),
      keywords: this.extractKeywords(allText)
    };
  }

  /**
   * Build prompt for AI title generation
   */
  private buildTitlePrompt(context: ConversationContext): string {
    let prompt = 'Lag en kort tittel for denne samtalen basert på innholdet:\n\n';
    
    if (context.firstUserMessage) {
      prompt += `Første melding: "${context.firstUserMessage.substring(0, 200)}"\n`;
    }
    
    if (context.lastUserMessage && context.lastUserMessage !== context.firstUserMessage) {
      prompt += `Siste melding: "${context.lastUserMessage.substring(0, 200)}"\n`;
    }

    if (context.keywords.length > 0) {
      prompt += `Nøkkelord: ${context.keywords.slice(0, 5).join(', ')}\n`;
    }

    prompt += '\nLag en kort, beskrivende tittel (maks 30 tegn) på norsk:';
    
    return prompt;
  }

  /**
   * Generate title from keyword analysis
   */
  private generateFromKeywords(context: ConversationContext): string {
    if (context.keywords.length === 0) {
      return this.generateFromFirstMessage(context.firstUserMessage);
    }

    // Use most relevant keywords
    const topKeywords = context.keywords
      .slice(0, 3)
      .map(word => this.capitalizeWord(word));

    let title = topKeywords.join(' ');

    // Add context based on content type
    if (context.hasQuestions) {
      title = `Spørsmål: ${title}`;
    } else if (context.hasTechnicalTerms) {
      title = `Tech: ${title}`;
    }

    return this.sanitizeTitle(title);
  }

  /**
   * Generate title from first message
   */
  private generateFromFirstMessage(firstMessage: string): string {
    if (!firstMessage || firstMessage.length === 0) {
      return 'Ny samtale';
    }

    // Extract first meaningful words
    const words = firstMessage
      .toLowerCase()
      .replace(/[^\w\sæøå]/g, ' ')
      .split(/\s+/)
      .filter(word => word.length > 2)
      .filter(word => !this.isStopWord(word))
      .slice(0, 3);

    if (words.length === 0) {
      return 'Samtale';
    }

    return this.sanitizeTitle(
      words.map(word => this.capitalizeWord(word)).join(' ')
    );
  }

  /**
   * Extract keywords from text
   */
  private extractKeywords(text: string): string[] {
    const cleanText = text.toLowerCase().replace(/[^\w\sæøå]/g, ' ');
    const words = cleanText.split(/\s+/).filter(word => word.length > 2);
    
    // Remove stop words
    const filteredWords = words.filter(word => !this.isStopWord(word));
    
    // Count word frequency
    const wordCount: Record<string, number> = {};
    filteredWords.forEach(word => {
      wordCount[word] = (wordCount[word] || 0) + 1;
    });

    // Sort by frequency and return top keywords
    return Object.entries(wordCount)
      .sort(([,a], [,b]) => b - a)
      .slice(0, 10)
      .map(([word]) => word);
  }

  /**
   * Check if text contains questions
   */
  private containsQuestions(text: string): boolean {
    const questionWords = ['hva', 'hvem', 'hvor', 'hvordan', 'hvorfor', 'når', 'kan', 'vil', 'skal', 'what', 'how', 'why', 'when', 'where', 'who'];
    const lowerText = text.toLowerCase();
    return questionWords.some(word => lowerText.includes(word)) || text.includes('?');
  }

  /**
   * Check if text contains technical terms
   */
  private containsTechnicalTerms(text: string): boolean {
    const techTerms = ['api', 'database', 'server', 'kod', 'code', 'function', 'variable', 'array', 'object', 'class', 'interface', 'component', 'service', 'docker', 'kubernetes', 'azure', 'aws', 'react', 'node', 'typescript', 'javascript', 'python', 'sql', 'json', 'xml', 'http', 'https', 'endpoint', 'microservice'];
    const lowerText = text.toLowerCase();
    return techTerms.some(term => lowerText.includes(term));
  }

  /**
   * Simple language detection
   */
  private detectLanguage(text: string): string {
    const norwegianWords = ['og', 'i', 'å', 'for', 'på', 'med', 'av', 'til', 'er', 'som', 'ikke', 'har', 'kan', 'vil', 'skal', 'det', 'en', 'et', 'den', 'de', 'så', 'når', 'hvor', 'hva', 'om'];
    const englishWords = ['the', 'and', 'is', 'in', 'to', 'of', 'a', 'that', 'it', 'with', 'for', 'as', 'was', 'on', 'are', 'you', 'have', 'be', 'at', 'this', 'but', 'his', 'by', 'from'];
    
    const lowerText = text.toLowerCase();
    const norwegianCount = norwegianWords.filter(word => lowerText.includes(word)).length;
    const englishCount = englishWords.filter(word => lowerText.includes(word)).length;
    
    return norwegianCount > englishCount ? 'no' : 'en';
  }

  /**
   * Check if word is a stop word
   */
  private isStopWord(word: string): boolean {
    const stopWords = [
      // Norwegian
      'og', 'i', 'å', 'for', 'på', 'med', 'av', 'til', 'er', 'som', 'ikke', 'har', 'kan', 'vil', 'skal', 'det', 'en', 'et', 'den', 'de', 'så', 'når', 'hvor', 'hva', 'om', 'hvis', 'men', 'eller', 'jeg', 'du', 'vi', 'han', 'hun', 'dem', 'dette', 'disse', 'alle', 'noen', 'bare', 'også', 'der', 'her', 'fram', 'opp', 'ned', 'ut', 'inn',
      // English
      'the', 'and', 'is', 'in', 'to', 'of', 'a', 'that', 'it', 'with', 'for', 'as', 'was', 'on', 'are', 'you', 'have', 'be', 'at', 'this', 'but', 'his', 'by', 'from', 'they', 'we', 'say', 'her', 'she', 'or', 'an', 'will', 'my', 'one', 'all', 'would', 'there', 'their'
    ];
    
    return stopWords.includes(word.toLowerCase());
  }

  /**
   * Capitalize first letter of word
   */
  private capitalizeWord(word: string): string {
    return word.charAt(0).toUpperCase() + word.slice(1);
  }

  /**
   * Sanitize and validate title
   */
  private sanitizeTitle(title: string): string {
    // Clean up the title
    let cleaned = title
      .trim()
      .replace(/^["']|["']$/g, '') // Remove quotes
      .replace(/[^\w\s\-æøåÆØÅ]/g, '') // Keep only letters, numbers, spaces, hyphens, and Norwegian chars
      .replace(/\s+/g, ' ') // Normalize spaces
      .trim();

    // Ensure reasonable length
    if (cleaned.length > 30) {
      cleaned = cleaned.substring(0, 27) + '...';
    }

    // Ensure minimum length
    if (cleaned.length < 3) {
      cleaned = 'Samtale';
    }

    return cleaned;
  }

  /**
   * Get fallback title when all else fails
   */
  private getFallbackTitle(sessionId: string): TitleGenerationResponse {
    const shortId = sessionId.substring(0, 8);
    return {
      title: `Samtale ${shortId}`,
      confidence: 0.1,
      fallbackUsed: true
    };
  }

  /**
   * Utility delay function
   */
  private delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }
}

interface ConversationContext {
  messageCount: number;
  userMessageCount: number;
  firstUserMessage: string;
  lastUserMessage: string;
  allText: string;
  hasQuestions: boolean;
  hasTechnicalTerms: boolean;
  language: string;
  keywords: string[];
}
