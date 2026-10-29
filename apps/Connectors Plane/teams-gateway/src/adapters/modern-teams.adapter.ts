import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import {
  Application,
  ApplicationBuilder,
  TeamsAdapter,
  TurnState
} from '@microsoft/teams-ai';
import {
  CloudAdapter,
  ConfigurationBotFrameworkAuthentication,
  ConfigurationServiceClientCredentialFactory
} from 'botbuilder';
import { MemoryStorage } from 'botbuilder';
import { AdaptiveCard } from 'adaptivecards';
import { Request, Response } from 'express';
import { BotConfigService } from '../config/bot.config';
import { AiOrchestratorClient } from '../ai/ai-orchestrator.client';
import { WebSocketClientService } from '../ai/websocket-client.service';
import { TeamsAudioService } from '../ai/teams-audio.service';
import { TeamsUserService } from '../auth/teams-user.service';
import { LiveTranslationService } from '../services/live-translation.service';
import { SmartModeService } from '../services/smart-mode.service';
import { SessionCommandService } from '../services/session-command.service';
import { AdaptiveCardService } from '../services/adaptive-card.service';
import { MessageRole } from '../entities/session-message.entity';

@Injectable()
export class ModernTeamsAdapter {
  private readonly logger = new Logger(ModernTeamsAdapter.name);
  private app: Application<TurnState>;
  // We allow either TeamsAdapter or CloudAdapter (runtime decision while debugging auth issues)
  private adapter: any;

  constructor(
    private configService: ConfigService,
    private botConfigService: BotConfigService,
    private aiOrchestratorClient: AiOrchestratorClient,
    private webSocketClient: WebSocketClientService,
    private teamsAudioService: TeamsAudioService,
    private teamsUserService: TeamsUserService,
    private liveTranslationService: LiveTranslationService,
    private smartModeService: SmartModeService,
    private sessionCommandService: SessionCommandService,
    private adaptiveCardService: AdaptiveCardService
  ) {
    this.initializeTeamsAIApplication();
    this.setupRealtimeEventHandlers();
  }

  private initializeTeamsAIApplication(): void {
    const config = this.botConfigService.getBotConfig();
    
    this.logger.log('🤖 Initializing Teams AI library application...');

    // Validate that credentials are available
    if (!config.appId || !config.appPassword) {
      throw new Error('Microsoft App credentials are required but not configured');
    }

    // Set environment variables explicitly (some underlying libs only read env at construction time)
    process.env.MicrosoftAppId = config.appId;
    process.env.MicrosoftAppPassword = config.appPassword;
    // Preserve provided MicrosoftAppType (e.g. SingleTenant) else default to MultiTenant
    process.env.MicrosoftAppType = process.env.MicrosoftAppType || process.env.MICROSOFT_APP_TYPE || 'MultiTenant';
    // Support multiple possible tenant id variable names
    process.env.MicrosoftAppTenantId =
      process.env.MicrosoftAppTenantId ||
      process.env.MICROSOFT_APP_TENANT_ID ||
      process.env.AZURE_TENANT_ID ||
      process.env.AZURE_DIRECTORY_TENANT_ID ||
      '';

    this.logger.log(
      `🔐 Credential debug (pre-adapter): appId len=${config.appId.length}, password len=${config.appPassword?.length || 0}, env.Id len=${(process.env.MicrosoftAppId||'').length}, env.Pw len=${(process.env.MicrosoftAppPassword||'').length}, appType=${process.env.MicrosoftAppType}, tenantId set=${!!process.env.MicrosoftAppTenantId}`
    );
    if (!process.env.MicrosoftAppTenantId && (process.env.MicrosoftAppType === 'SingleTenant')) {
      this.logger.warn('⚠️ SingleTenant selected but MicrosoftAppTenantId not resolved from MICROSOFT_APP_TENANT_ID / AZURE_TENANT_ID. Will force MultiTenant fallback which can cause AADSTS700016.');
    } else {
      this.logger.log(`🏢 Tenant resolution: type=${process.env.MicrosoftAppType} tenantId=${process.env.MicrosoftAppTenantId?.slice(0,8) || 'NONE'}…`);
    }

    const appType = process.env.MicrosoftAppType as 'SingleTenant' | 'MultiTenant' | 'UserAssignedMSI' | 'ManagedIdentity' | undefined;
    const tenantId = process.env.MicrosoftAppTenantId;
    if (appType === 'SingleTenant' && !tenantId) {
      this.logger.error('❌ MicrosoftAppType=SingleTenant but no tenant id found (MicrosoftAppTenantId / MICROSOFT_APP_TENANT_ID / AZURE_TENANT_ID). Authentication will fail.');
    }

    // Attempt CloudAdapter first (matches official JS samples). If it fails, fall back to TeamsAdapter.
    try {
      const credentialFactoryOptions: any = {
        MicrosoftAppId: config.appId,
        MicrosoftAppPassword: config.appPassword,
        MicrosoftAppType: appType || 'MultiTenant'
      };
      if (appType === 'SingleTenant' && tenantId) {
        credentialFactoryOptions.MicrosoftAppTenantId = tenantId;
      } else if (appType === 'SingleTenant' && !tenantId) {
        // Force MultiTenant to avoid incorrect authority pointing at botframework.com directory
        credentialFactoryOptions.MicrosoftAppType = 'MultiTenant';
        this.logger.warn('🔄 Downgrading auth to MultiTenant due to missing tenant id; expect potential AADSTS700016 if app is single-tenant only.');
      }
      const botFrameworkAuthentication = new ConfigurationBotFrameworkAuthentication(
        {},
        new ConfigurationServiceClientCredentialFactory(credentialFactoryOptions)
      );
      this.adapter = new CloudAdapter(botFrameworkAuthentication);
      this.logger.log(`☁️ Using CloudAdapter for Bot Framework auth (type=${credentialFactoryOptions.MicrosoftAppType}${credentialFactoryOptions.MicrosoftAppTenantId ? ', tenantId=' + credentialFactoryOptions.MicrosoftAppTenantId.slice(0,8)+'…' : ''})`);
    } catch (cloudErr) {
      this.logger.error(`⚠️ Failed to initialize CloudAdapter, falling back to TeamsAdapter: ${cloudErr.message}`);
      this.adapter = new TeamsAdapter({
        MicrosoftAppId: config.appId
      });
      this.logger.log('🤖 Fallback: Using TeamsAdapter');
    }

    // Configure storage for conversation state
    const storage = new MemoryStorage();

    // Build the Teams AI application with simplified configuration
    this.app = new ApplicationBuilder<TurnState>()
      .withStorage(storage)
      .withTurnStateFactory(() => new TurnState())
      .build();

    // Register message handlers
    this.registerMessageHandlers();
    
    this.logger.log('✅ Teams AI application initialized successfully');
  }

  private registerMessageHandlers(): void {
    // Handle slash commands first
    this.registerSlashCommands();
    
    // Handle all messages with pattern matching
    this.app.message(/.*/i, async (context, state) => {
      const userMessage = context.activity.text?.trim();
      const attachments = context.activity.attachments;
      
      if (!userMessage && (!attachments || attachments.length === 0)) {
        return; // Let framework handle empty messages
      }

      // Skip if this is a slash command (already handled)
      if (userMessage?.startsWith('/')) {
        return;
      }

      this.logger.log(`📥 Message from ${context.activity.from.name}: ${userMessage || '[attachments only]'}`);

      try {
        // Extra runtime credential diagnostics on first user message
        this.logger.log(
          `🔎 Runtime credential check: env.MicrosoftAppId len=${(process.env.MicrosoftAppId||'').length}, env.MicrosoftAppPassword present=${!!process.env.MicrosoftAppPassword}`
        );
        // Extract Microsoft Teams user context
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        if (!userContext || !this.teamsUserService.isValidTeamsUser(userContext)) {
          this.logger.error('❌ Failed to extract valid user context from Teams');
          await context.sendActivity('Beklager, jeg kunne ikke autentisere din Microsoft 365-konto. Vennligst prøv igjen.');
          return;
        }

        // Log user context (with privacy protection)
        const sanitizedEmail = this.teamsUserService.sanitizeEmailForLogging(userContext.email);
        this.logger.log(`👤 Processing request for user: ${sanitizedEmail} (${userContext.displayName})`);

        // Send typing indicator
        await context.sendActivities([{ type: 'typing' }]);

        // Process file attachments if present
        let processedAttachments = [];
        let audioTranscription = '';
        
        if (attachments && attachments.length > 0) {
          try {
            // Check for audio attachments first
            const audioAttachments = attachments.filter(att => 
              att.contentType?.startsWith('audio/') || 
              att.name?.match(/\.(wav|mp3|m4a|ogg|webm)$/i)
            );

            if (audioAttachments.length > 0 && this.teamsAudioService.isConnected()) {
              this.logger.log(`🎤 Processing ${audioAttachments.length} audio attachment(s)`);
              
              for (const audioAtt of audioAttachments) {
                try {
                  await context.sendActivity('🎤 Behandler lydmelding...');
                  
                  // In a real implementation, you would download and process the audio
                  // For now, we'll simulate audio processing
                  const transcriptionResult = await this.teamsAudioService.transcribeAudio({
                    audioData: 'simulated_audio_base64', // In real implementation: download and convert to base64
                    format: audioAtt.contentType?.split('/')[1] || 'wav',
                    userId: context.activity.from.id,
                    userName: context.activity.from.name,
                    conversationId: context.activity.conversation.id,
                    requestType: 'transcription',
                  });

                  if (transcriptionResult.success && transcriptionResult.text) {
                    audioTranscription += `🎤 ${transcriptionResult.text} `;
                    await context.sendActivity(`🎤 **Lydtranskription:** ${transcriptionResult.text}`);
                  } else {
                    await context.sendActivity(`❌ Kunne ikke behandle lydmelding: ${transcriptionResult.error}`);
                  }
                } catch (audioError) {
                  this.logger.error(`Failed to process audio attachment: ${audioError.message}`);
                  await context.sendActivity('❌ Feil ved behandling av lydmelding.');
                }
              }
            }

            // Process non-audio attachments
            const nonAudioAttachments = attachments.filter(att => 
              !att.contentType?.startsWith('audio/') && 
              !att.name?.match(/\.(wav|mp3|m4a|ogg|webm)$/i)
            );

            if (nonAudioAttachments.length > 0) {
              processedAttachments = await this.processTeamsAttachments(nonAudioAttachments, context);
            }
            
          } catch (attachmentError) {
            this.logger.error('❌ Failed to process attachments:', attachmentError);
            await context.sendActivity('⚠️ Noen vedlegg kunne ikke behandles, men jeg kan fortsatt hjelpe med meldingen din.');
          }
        }

        // Use AI Orchestrator for complex operations (preserves existing business logic)
        const finalMessage = audioTranscription 
          ? `${userMessage || ''} ${audioTranscription}`.trim()
          : userMessage || 'Vennligst analyser de vedlagte filene.';

        // Add user message to current session
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = await this.sessionCommandService.addMessageToCurrentSession(
          userId, 
          finalMessage, 
          MessageRole.USER
        );

        const aiResponse = await this.aiOrchestratorClient.getAiResponse({
          message: finalMessage,
          userId: userContext.email,
          userName: userContext.displayName,
          userEmail: userContext.email,
          conversationId: context.activity.conversation.id,
          channelId: context.activity.channelId,
          teamId: context.activity.conversation.conversationType === 'channel' 
            ? context.activity.channelData?.team?.id 
            : undefined,
          tenantId: userContext.tenantId,
          objectId: userContext.objectId,
          attachments: processedAttachments
        });

        // Send the AI response with security context
        let responseText = aiResponse.response;
        
        // Add security notices for restricted content
        if (aiResponse.securityLevel === 'restricted') {
          responseText += '\n\n🔒 *Noe innhold ble filtrert basert på dine tilgangstillatelser.*';
        } else if (aiResponse.securityLevel === 'confidential') {
          responseText += '\n\n🔒 *Tilgang begrenset - begrenset informasjon tilgjengelig.*';
        }

        await context.sendActivity(responseText);

        // Add assistant response to session 
        await this.sessionCommandService.addMessageToCurrentSession(
          userId,
          responseText,
          MessageRole.ASSISTANT
        );

        // Log security information if violations occurred
        if (aiResponse.accessViolations && aiResponse.accessViolations.length > 0) {
          this.logger.log(`🛡️ Access violations for ${sanitizedEmail}: ${aiResponse.accessViolations.join(', ')}`);
        }

        this.logger.log(`✅ AI response sent for user ${sanitizedEmail}: ${aiResponse.model} (${aiResponse.usage?.totalTokens || 0} tokens) - Security: ${aiResponse.securityLevel || 'public'}`);

      } catch (error) {
        this.logger.error(`❌ Error processing message: ${error.message}`, error.stack);
        
        // Send fallback response in Norwegian
        try {
          await context.sendActivity("Beklager, jeg har problemer med å behandle forespørselen din akkurat nå. Vennligst prøv igjen senere.");
        } catch (fallbackError) {
          this.logger.error(`❌ Failed to send fallback response: ${fallbackError.message}`);
        }
      }
    });

    // Handle members added (welcome message)
    this.app.conversationUpdate('membersAdded', async (context, state) => {
      const membersAdded = context.activity.membersAdded;
      
      if (membersAdded) {
        for (const member of membersAdded) {
          if (member.id !== context.activity.recipient.id) {
            const welcomeText = `👋 **Velkommen til Aquatiq Chat!**

Jeg er din AI-assistent og kan hjelpe deg med:
• 💬 Svare på spørsmål på norsk
• 🎨 Generere bilder med AI
• 📋 Utføre enkle oppgaver

Hvordan kan jeg hjelpe deg i dag? Skriv /hjelp for en liste over kommandoer og funksjoner.`;
            await context.sendActivity(welcomeText);
          }
        }
      }
    });

    // Handle reset command (legacy support)
    this.app.message('/reset', async (context, state) => {
      // Clear conversation state
      state.conversation = {} as any;
      state.user = {} as any;
      state.temp.input = '';
      
      await context.sendActivity('🔄 Samtaletilstand er nullstilt. Hvordan kan jeg hjelpe deg?');
    });

    // Audio Commands
    this.app.message('/voice', async (context, state) => {
      const userId = context.activity.from.id;
      const sessionId = this.teamsAudioService.getRealtimeSession(userId);
      
      if (!sessionId) {
        await context.sendActivity('🎤 Starter stemmesamtale med AI...');
        const newSessionId = await this.teamsAudioService.startRealtimeConversation(userId);
        if (newSessionId) {
          await context.sendActivity('✅ Stemmesamtale startet! Du kan nå sende lydmeldinger.');
        } else {
          await context.sendActivity('❌ Kunne ikke starte stemmesamtale. Prøv igjen senere.');
        }
      } else {
        await context.sendActivity(`🎤 Du har allerede en aktiv stemmesamtale (${sessionId}). Bruk /endvoice for å avslutte.`);
      }
    });

    this.app.message('/endvoice', async (context, state) => {
      const userId = context.activity.from.id;
      const success = await this.teamsAudioService.endRealtimeConversation(userId);
      
      if (success) {
        await context.sendActivity('🔇 Stemmesamtale avsluttet.');
      } else {
        await context.sendActivity('❌ Kunne ikke avslutte stemmesamtale.');
      }
    });

    this.app.message('/speak', async (context, state) => {
      const text = context.activity.text?.replace('/speak', '').trim();
      if (!text) {
        await context.sendActivity('💬 Skriv teksten du vil at jeg skal si. Eksempel: /speak Hei, dette er en test');
        return;
      }

      try {
        await context.sendActivity('🔊 Konverterer til tale...');
        const result = await this.teamsAudioService.synthesizeText(text, context.activity.from.id, 'nb-NO');
        
        if (result.success && result.audioData) {
          await context.sendActivity('✅ Tale generert! (Audio-filen ville blitt sendt i en ekte implementasjon)');
        } else {
          await context.sendActivity(`❌ Kunne ikke generere tale: ${result.error}`);
        }
      } catch (error) {
        this.logger.error(`Failed to synthesize speech: ${error.message}`);
        await context.sendActivity('❌ Feil ved generering av tale.');
      }
    });

    this.app.message('/translate', async (context, state) => {
      const text = context.activity.text?.replace('/translate', '').trim();
      if (!text) {
        await context.sendActivity('🌐 Skriv teksten du vil oversette. Eksempel: /translate Hello world');
        return;
      }

      try {
        await context.sendActivity('🌐 Oversetter...');
        const result = await this.teamsAudioService.translateText(text, 'auto', 'nb');
        
        if (result.success) {
          const response = `🌐 **Oversettelse:**\n📝 Original: ${result.text}\n✅ Oversatt: ${result.translatedText}`;
          await context.sendActivity(response);
        } else {
          await context.sendActivity(`❌ Kunne ikke oversette: ${result.error}`);
        }
      } catch (error) {
        this.logger.error(`Failed to translate text: ${error.message}`);
        await context.sendActivity('❌ Feil ved oversettelse.');
      }
    });

    this.app.message('/audiostatus', async (context, state) => {
      const status = this.teamsAudioService.getServiceStatus();
      const userId = context.activity.from.id;
      const sessionId = this.teamsAudioService.getRealtimeSession(userId);
      
      const statusMessage = `🎤 **Audio Service Status:**
🔗 WebSocket: ${status.connected ? '✅ Tilkoblet' : '❌ Frakoblet'}
🎙️ Stemme-til-tekst: ${status.availableFeatures.speechToText ? '✅' : '❌'}
🔊 Tekst-til-stemme: ${status.availableFeatures.textToSpeech ? '✅' : '❌'}
🌐 Oversettelse: ${status.availableFeatures.translation ? '✅' : '❌'}
🤖 Sanntids-AI: ${status.availableFeatures.realtimeAI ? '✅' : '❌'}
📱 Aktive sesjoner: ${status.activeRealtimeSessions}
👤 Din sesjon: ${sessionId ? '✅ Aktiv' : '❌ Ingen'}`;

      await context.sendActivity(statusMessage);
    });

    this.logger.log('📋 Message handlers registered successfully');
  }

  /**
   * Setup event handlers for real-time services
   */
  private setupRealtimeEventHandlers(): void {
    // Live Translation Event Handlers
    this.liveTranslationService.on('translationResult', (result) => {
      this.logger.log(`🌐 Translation result: ${result.originalText} → ${result.translatedText}`);
      // The translation result will be sent to Teams via the active session
    });

    this.liveTranslationService.on('sessionStarted', (data) => {
      this.logger.log(`🎙️ Live translation session started: ${data.sessionId}`);
    });

    this.liveTranslationService.on('sessionEnded', (data) => {
      this.logger.log(`🛑 Live translation session ended: ${data.sessionId}`);
    });

    // Smart Mode Event Handlers
    this.smartModeService.on('smartResponse', (response) => {
      this.logger.log(`🧠 Smart response: ${response.content}`);
      // The smart response will be sent to Teams via the active session
    });

    this.smartModeService.on('meetingInsight', (insight) => {
      this.logger.log(`💡 Meeting insight: ${insight.insight}`);
    });

    this.logger.log('🔄 Real-time event handlers setup complete');
  }

  /**
   * Register slash command handlers
   */
  private registerSlashCommands(): void {
    // /hjelp - Show help with commands
    this.app.message('/hjelp', async (context, state) => {
      const helpText = `
🤖 **Aquatiq Chat - Tilgjengelige kommandoer:**

**📋 Grunnleggende kommandoer:**
**/hjelp** - Vis denne hjelpe-menyen
**/status** - Sjekk systemstatus og tilgjengelige funksjoner
**/clear** - Nullstill samtalekontekst

**🎨 AI-funksjoner:**
**/bilde <beskrivelse>** - Generer bilde med AI
**/faq <spørsmål>** - Søk kun i FAQ-dokumenter
**/preferanser <tekst>** - Lagre personlige instruksjoner
**/instruksjoner** - Vis dine gjeldende instruksjoner

**🌐 Sanntidsfunksjoner:**
**/oversett <start|stopp>** - Start/stopp live oversettelse
**/smart <start|stopp>** - Start/stopp Smart Mode for møter
**/møte <sammendrag|handlinger|neste>** - Møteassistanse

**💬 Tilbakemelding:**
**/feedback <tekst>** - Gi tilbakemelding

**Eksempler:**
• /bilde En moderne logo for Aquatiq
• /oversett start - Start live oversettelse nb-NO → en-US
• /smart start - Aktiver Smart Mode for møtet
• /møte sammendrag - Få sammendrag av pågående møte

Du kan også bare skrive vanlige spørsmål uten kommandoer!`;
      
      await context.sendActivity(helpText);
    });

    // /status - System health check with real-time capabilities
    this.app.message('/status', async (context, state) => {
      try {
        const healthCheck = await this.aiOrchestratorClient.healthCheck();
        const liveTranslationAvailable = this.liveTranslationService.isAvailable();
        const smartModeAvailable = this.smartModeService.isAvailable();
        
        const statusEmoji = healthCheck ? '✅' : '❌';
        const statusText = healthCheck ? 'Operativ' : 'Ikke tilgjengelig';
        
        const statusMessage = `
🔧 **Systemstatus:**

**AI Orchestrator:** ${statusEmoji} ${statusText}
**Teams Gateway:** ✅ Operativ
**Språk:** 🇳🇴 Norsk (standard)

**🎯 Tilgjengelige funksjoner:**
• 💬 **Chat:** ${healthCheck ? '✅' : '❌'} Grunnleggende samtaler
• 🎨 **Bildegenerering:** ${healthCheck ? '✅' : '❌'} DALL-E 3
• 🌐 **Live oversettelse:** ${liveTranslationAvailable ? '✅' : '❌'} ${liveTranslationAvailable ? 'Azure Speech' : 'Ikke tilgjengelig'}
• 🤖 **Smart Mode:** ${smartModeAvailable ? '✅' : '❌'} ${smartModeAvailable ? 'OpenAI Realtime' : 'Ikke tilgjengelig'}

**Støttede språk:** ${liveTranslationAvailable ? this.liveTranslationService.getSupportedLanguages().join(', ') : 'N/A'}

**Tid:** ${new Date().toLocaleString('nb-NO')}

*Bruk /hjelp for å se alle kommandoer.*`;
        
        await context.sendActivity(statusMessage);
      } catch (error) {
        await context.sendActivity('❌ Kunne ikke hente fullstendig systemstatus. Grunnleggende chat er tilgjengelig.');
      }
    });

    // /clear - Clear conversation context
    this.app.message('/clear', async (context, state) => {
      // Clear conversation state
      state.conversation = {} as any;
      state.user = {} as any;
      state.temp.input = '';
      
      const conversationId = context.activity.conversation.id;
      
      try {
        // Also clear conversation in AI Orchestrator
        await this.aiOrchestratorClient.clearConversation(conversationId);
        
        await context.sendActivity('🔄 Samtalekontekst er nullstilt. Hvordan kan jeg hjelpe deg?');
      } catch (error) {
        this.logger.error('Failed to clear conversation in AI Orchestrator:', error);
        await context.sendActivity('🔄 Samtalekontekst er nullstilt lokalt. Hvordan kan jeg hjelpe deg?');
      }
    });

    // /preferanser - Save user preferences
    this.app.message(/^\/preferanser\s+(.+)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/preferanser\s+(.+)/i);
      const preferences = match ? match[1].trim() : '';
      
      if (!preferences) {
        await context.sendActivity('Vennligst spesifiser dine preferanser.\n\n**Eksempel:** /preferanser Jeg foretrekker korte svar på norsk');
        return;
      }

      try {
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        
        // Send preferences to AI Orchestrator
        const response = await this.aiOrchestratorClient.getAiResponse({
          message: `SYSTEM: Save user preferences: ${preferences}`,
          userId: userContext.email,
          conversationId: context.activity.conversation.id,
          systemPrompt: 'Save the user preferences for future conversations.'
        });
        
        await context.sendActivity(`✅ Dine preferanser er lagret:\n\n"${preferences}"\n\nDisse vil bli brukt i fremtidige samtaler.`);
      } catch (error) {
        await context.sendActivity('❌ Kunne ikke lagre preferanser. Prøv igjen senere.');
      }
    });

    // /instruksjoner - Show current user instructions
    this.app.message('/instruksjoner', async (context, state) => {
      try {
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        
        const response = await this.aiOrchestratorClient.getAiResponse({
          message: 'SYSTEM: Show current user preferences and instructions',
          userId: userContext.email,
          conversationId: context.activity.conversation.id,
          systemPrompt: 'Show the user their current saved preferences and instructions.'
        });
        
        await context.sendActivity(`📋 **Dine gjeldende instruksjoner:**\n\n${response.response}`);
      } catch (error) {
        this.logger.error('Failed to get user instructions:', error);
        await context.sendActivity('❌ Kunne ikke hente instruksjoner. Ingen preferanser er lagret ennå.');
      }
    });

    // /bilde - Generate image with AI
    this.app.message(/^\/bilde\s+(.+)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/bilde\s+(.+)/i);
      const prompt = match ? match[1].trim() : '';
      
      if (!prompt) {
        await context.sendActivity('Vennligst beskriv bildet du vil generere.\n\n**Eksempel:** /bilde En moderne logo for Aquatiq med blå farger');
        return;
      }

      try {
        await context.sendActivities([{ type: 'typing' }]);
        
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        
        const response = await this.aiOrchestratorClient.getAiResponse({
          message: `GENERATE_IMAGE: ${prompt}`,
          userId: userContext.email,
          conversationId: context.activity.conversation.id,
          systemPrompt: 'Generate an image based on the user prompt using DALL-E.'
        });
        
        await context.sendActivity(`🎨 **Bilde generert:**\n\n${response.response}`);
      } catch (error) {
        this.logger.error('Failed to generate image:', error);
        await context.sendActivity('❌ Kunne ikke generere bilde. Prøv igjen senere eller kontakt administrator.');
      }
    });

    // /faq - Search only FAQ documents
    this.app.message(/^\/faq\s+(.+)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/faq\s+(.+)/i);
      const question = match ? match[1].trim() : '';
      
      if (!question) {
        await context.sendActivity('Vennligst still et spørsmål.\n\n**Eksempel:** /faq Hvordan bestiller jeg ferie?');
        return;
      }

      try {
        await context.sendActivities([{ type: 'typing' }]);
        
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        
        const response = await this.aiOrchestratorClient.getAiResponse({
          message: `FAQ_SEARCH: ${question}`,
          userId: userContext.email,
          conversationId: context.activity.conversation.id,
          systemPrompt: 'Search only in FAQ documents and provide relevant answers in Norwegian.'
        });
        
        await context.sendActivity(`📚 **FAQ-søk:**\n\n${response.response}`);
      } catch (error) {
        this.logger.error('Failed to search FAQ:', error);
        await context.sendActivity('❌ Kunne ikke søke i FAQ. Prøv igjen senere.');
      }
    });

    // /feedback - Collect user feedback
    this.app.message(/^\/feedback\s+(.+)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/feedback\s+(.+)/i);
      const feedback = match ? match[1].trim() : '';
      
      if (!feedback) {
        await context.sendActivity('Vennligst gi din tilbakemelding.\n\n**Eksempel:** /feedback Svaret var veldig nyttig, men kunne vært kortere');
        return;
      }

      try {
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        
        // Log feedback for analysis
        this.logger.log(`📝 Feedback from ${this.teamsUserService.sanitizeEmailForLogging(userContext.email)}: ${feedback}`);
        
        // Send feedback to AI Orchestrator for storage
        await this.aiOrchestratorClient.getAiResponse({
          message: `FEEDBACK: ${feedback}`,
          userId: userContext.email,
          conversationId: context.activity.conversation.id,
          systemPrompt: 'Store this user feedback for system improvement.'
        });
        
        await context.sendActivity('✅ Takk for tilbakemeldingen! Den hjelper oss å forbedre tjenesten.');
      } catch (error) {
        this.logger.error('Failed to save feedback:', error);
        await context.sendActivity('✅ Takk for tilbakemeldingen! (Den ble lagret lokalt)');
      }
    });

    // /oversett - Live translation controls
    this.app.message(/^\/oversett\s+(start|stopp|status)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/oversett\s+(start|stopp|status)/i);
      const action = match ? match[1].toLowerCase() : '';
      
      try {
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        const conversationId = context.activity.conversation.id;

        if (action === 'start') {
          if (!this.liveTranslationService.isAvailable()) {
            await context.sendActivity('❌ Live oversettelse er ikke tilgjengelig. Sjekk systemstatus med /status');
            return;
          }

          const existingSession = this.liveTranslationService.getActiveSessionByConversation(conversationId);
          if (existingSession) {
            await context.sendActivity(`⚠️ Live oversettelse er allerede aktiv for denne samtalen (${existingSession.sessionId})`);
            return;
          }

          const session = await this.liveTranslationService.startTranslationSession(
            userContext.email,
            conversationId
          );

          await context.sendActivity(`🌐 **Live oversettelse startet!**

**Session ID:** ${session.sessionId}
**Fra:** ${session.sourceLanguage} (Norsk)
**Til:** ${session.targetLanguage} (Engelsk)
**Motor:** ${session.engine.toUpperCase()}

Snakk nå, og alt vil bli oversatt i sanntid. Bruk **/oversett stopp** for å avslutte.`);

        } else if (action === 'stopp') {
          const session = this.liveTranslationService.getActiveSessionByConversation(conversationId);
          if (!session) {
            await context.sendActivity('⚠️ Ingen aktiv oversettelse-session funnet for denne samtalen.');
            return;
          }

          await this.liveTranslationService.endTranslationSession(session.sessionId);
          await context.sendActivity(`🛑 **Live oversettelse stoppet**

Session ${session.sessionId} avsluttet. Takk for at du brukte live oversettelse!`);

        } else if (action === 'status') {
          const session = this.liveTranslationService.getActiveSessionByConversation(conversationId);
          if (!session) {
            await context.sendActivity('📊 **Oversettelse status:** Ingen aktiv session');
            return;
          }

          const duration = Math.round((Date.now() - session.startTime.getTime()) / 1000 / 60);
          await context.sendActivity(`📊 **Live oversettelse status:**

**Session ID:** ${session.sessionId}
**Varighet:** ${duration} minutter
**Fra → Til:** ${session.sourceLanguage} → ${session.targetLanguage}
**Motor:** ${session.engine.toUpperCase()}
**Status:** ✅ Aktiv`);
        }

      } catch (error) {
        this.logger.error('Failed to handle live translation command:', error);
        await context.sendActivity('❌ Kunne ikke behandle oversettelse-kommando. Prøv igjen senere.');
      }
    });

    // /smart - Smart Mode controls
    this.app.message(/^\/smart\s+(start|stopp|status)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/smart\s+(start|stopp|status)/i);
      const action = match ? match[1].toLowerCase() : '';
      
      try {
        const userContext = await this.teamsUserService.extractUserFromTeamsContext(context);
        const conversationId = context.activity.conversation.id;

        if (action === 'start') {
          if (!this.smartModeService.isAvailable()) {
            await context.sendActivity('❌ Smart Mode er ikke tilgjengelig. Sjekk systemstatus med /status');
            return;
          }

          const existingSession = this.smartModeService.getActiveSessionByConversation(conversationId);
          if (existingSession) {
            await context.sendActivity(`⚠️ Smart Mode er allerede aktiv for denne samtalen (${existingSession.sessionId})`);
            return;
          }

          const session = await this.smartModeService.startSmartModeSession(
            userContext.email,
            conversationId,
            {
              meetingAssistance: true,
              realTimeChat: true,
              contextAware: true
            }
          );

          await context.sendActivity(`🤖 **Smart Mode aktivert!**

**Session ID:** ${session.sessionId}
**Funksjoner:**
• 🤝 **Møteassistanse:** ${session.features.meetingAssistance ? '✅' : '❌'}
• 💬 **Sanntids-chat:** ${session.features.realTimeChat ? '✅' : '❌'}
• 🧠 **Kontekst-bevisst:** ${session.features.contextAware ? '✅' : '❌'}

Smart Mode hjelper med møtesammendrag, handlingspointer og intelligente forslag. Bruk **/møte** kommandoer for assistanse.`);

        } else if (action === 'stopp') {
          const session = this.smartModeService.getActiveSessionByConversation(conversationId);
          if (!session) {
            await context.sendActivity('⚠️ Ingen aktiv Smart Mode session funnet for denne samtalen.');
            return;
          }

          await this.smartModeService.endSmartModeSession(session.sessionId);
          await context.sendActivity(`🛑 **Smart Mode deaktivert**

Session ${session.sessionId} avsluttet. Takk for at du brukte Smart Mode!`);

        } else if (action === 'status') {
          const session = this.smartModeService.getActiveSessionByConversation(conversationId);
          if (!session) {
            await context.sendActivity('📊 **Smart Mode status:** Ikke aktiv');
            return;
          }

          const duration = Math.round((Date.now() - session.startTime.getTime()) / 1000 / 60);
          await context.sendActivity(`📊 **Smart Mode status:**

**Session ID:** ${session.sessionId}
**Varighet:** ${duration} minutter
**Møteassistanse:** ${session.features.meetingAssistance ? '✅' : '❌'}
**Sanntids-chat:** ${session.features.realTimeChat ? '✅' : '❌'}
**Kontekst-bevisst:** ${session.features.contextAware ? '✅' : '❌'}
**Status:** ✅ Aktiv`);
        }

      } catch (error) {
        this.logger.error('Failed to handle Smart Mode command:', error);
        await context.sendActivity('❌ Kunne ikke behandle Smart Mode kommando. Prøv igjen senere.');
      }
    });

    // /møte - Meeting assistance commands
    this.app.message(/^\/møte\s+(sammendrag|handlinger|neste|innsikt)/i, async (context, state) => {
      const match = context.activity.text.match(/^\/møte\s+(sammendrag|handlinger|neste|innsikt)/i);
      const assistanceType = match ? match[1].toLowerCase() : '';
      
      try {
        const conversationId = context.activity.conversation.id;
        const session = this.smartModeService.getActiveSessionByConversation(conversationId);
        
        if (!session) {
          await context.sendActivity('⚠️ Smart Mode må være aktivert for møteassistanse. Bruk **/smart start** først.');
          return;
        }

        if (!session.features.meetingAssistance) {
          await context.sendActivity('⚠️ Møteassistanse er ikke aktivert for denne Smart Mode sessionen.');
          return;
        }

        await context.sendActivities([{ type: 'typing' }]);

        // Map Norwegian commands to English assistance types
        const assistanceMap = {
          'sammendrag': 'summary',
          'handlinger': 'action_items', 
          'neste': 'next_steps',
          'innsikt': 'insights'
        };

        const englishType = assistanceMap[assistanceType] as 'summary' | 'action_items' | 'next_steps' | 'insights';
        
        await this.smartModeService.requestMeetingAssistance(
          session.sessionId,
          englishType,
          {
            conversationId,
            requestedBy: context.activity.from.name,
            timestamp: new Date().toISOString()
          }
        );

        const responseMap = {
          'sammendrag': 'møtesammendrag',
          'handlinger': 'handlingspointer',
          'neste': 'neste steg',
          'innsikt': 'møteinnsikt'
        };

        await context.sendActivity(`🤝 **Møteassistanse:** Genererer ${responseMap[assistanceType]}...

Smart Mode analyserer samtalen og vil levere ${responseMap[assistanceType]} basert på det som er diskutert så langt.`);

      } catch (error) {
        this.logger.error('Failed to handle meeting assistance command:', error);
        await context.sendActivity('❌ Kunne ikke behandle møteassistanse-kommando. Prøv igjen senere.');
      }
    });

    // Session management commands
    this.registerSessionCommands();

    this.logger.log('⚡ Slash commands registered successfully');
  }

  /**
   * Register session management commands
   */
  private registerSessionCommands(): void {
    // Handle all session commands with pattern matching
    this.app.message(/^\/(?:ny|new|threads|samtaler|bytt|switch|endre|rename|arkiver|archive|slett|delete|pin|unpin|help)/, async (context, state) => {
      const userMessage = context.activity.text?.trim();
      if (!userMessage) return;

      try {
        // Get user ID for session management
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        
        // Check for help command
        if (userMessage.toLowerCase() === '/help' || userMessage.toLowerCase() === '/hjelp') {
          const helpText = this.sessionCommandService.getHelpText();
          await context.sendActivity(helpText);
          return;
        }

        // Process command through session command service
        const result = await this.sessionCommandService.handleCommand(userId, userMessage);
        
        if (!result) {
          // Not a recognized session command, let normal flow handle it
          return;
        }

        // Send response based on command result
        if (result.requiresResponse && result.data) {
          // Commands that return adaptive cards (like /threads)
          if (userMessage.toLowerCase().includes('threads') || userMessage.toLowerCase().includes('samtaler')) {
            const cardData = result.data;
            const adaptiveCard = this.adaptiveCardService.buildThreadSwitcherCard(cardData);
            
            await context.sendActivity({
              type: 'message',
              attachments: [{
                contentType: 'application/vnd.microsoft.card.adaptive',
                content: adaptiveCard
              }]
            });
          }
        } else {
          // Simple text responses
          await context.sendActivity(result.message);
        }

      } catch (error) {
        this.logger.error('Failed to handle session command:', error);
        await context.sendActivity('❌ Kunne ikke behandle kommandoen. Prøv igjen senere.');
      }
    });

    // Handle Adaptive Card actions  
    this.app.adaptiveCards.actionExecute('newSession', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const result = await this.sessionCommandService.handleNewSessionCommand(userId);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle newSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke opprette ny samtale.');
      }
    });

    this.app.adaptiveCards.actionExecute('switchSession', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        
        const result = await this.sessionCommandService.handleSwitchSessionCommand(userId, sessionId);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle switchSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke bytte samtale.');
      }
    });

    this.app.adaptiveCards.actionExecute('renameSession', async (context, state, data) => {
      try {
        const sessionId = data.sessionId;
        const currentTitle = data.currentTitle;
        
        return this.adaptiveCardService.buildRenameSessionCard(sessionId, currentTitle);
      } catch (error) {
        this.logger.error('Failed to handle renameSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke vise rename-dialog.');
      }
    });

    this.app.adaptiveCards.actionExecute('saveRename', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        const newTitle = context.activity.value?.newTitle;
        
        if (!newTitle || newTitle.trim() === '') {
          return this.adaptiveCardService.buildErrorCard('❌ Navn kan ikke være tomt.');
        }
        
        const result = await this.sessionCommandService.handleRenameSessionCommand(userId, sessionId, newTitle.trim());
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle saveRename action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke lagre nytt navn.');
      }
    });

    this.app.adaptiveCards.actionExecute('generateAiTitle', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        
        // Show loading message
        await context.sendActivity('🤖 Genererer AI-tittel basert på samtaleinnhold...');
        
        const result = await this.sessionCommandService.handleGenerateAiTitleCommand(userId, sessionId);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle generateAiTitle action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke generere AI-tittel.');
      }
    });

    this.app.adaptiveCards.actionExecute('archiveSession', async (context, state, data) => {
      try {
        const sessionId = data.sessionId;
        // Get session title for confirmation
        // For now, we'll just confirm directly
        return this.adaptiveCardService.buildConfirmationCard('archive', sessionId, 'denne samtalen');
      } catch (error) {
        this.logger.error('Failed to handle archiveSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke vise arkiverings-dialog.');
      }
    });

    this.app.adaptiveCards.actionExecute('confirm_archive', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        
        const result = await this.sessionCommandService.handleArchiveSessionCommand(userId, sessionId);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle confirm_archive action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke arkivere samtale.');
      }
    });

    this.app.adaptiveCards.actionExecute('pinSession', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        
        const result = await this.sessionCommandService.handlePinSessionCommand(userId, sessionId, true);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle pinSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke feste samtale.');
      }
    });

    this.app.adaptiveCards.actionExecute('unpinSession', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const sessionId = data.sessionId;
        
        const result = await this.sessionCommandService.handlePinSessionCommand(userId, sessionId, false);
        
        if (result.success) {
          return this.adaptiveCardService.buildSuccessCard(result.message);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle unpinSession action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke løsne samtale.');
      }
    });

    this.app.adaptiveCards.actionExecute('refreshSessions', async (context, state, data) => {
      try {
        const userId = context.activity.from.aadObjectId || context.activity.from.id;
        const result = await this.sessionCommandService.handleThreadsCommand(userId);
        
        if (result.success && result.data) {
          return this.adaptiveCardService.buildThreadSwitcherCard(result.data);
        } else {
          return this.adaptiveCardService.buildErrorCard(result.message);
        }
      } catch (error) {
        this.logger.error('Failed to handle refreshSessions action:', error);
        return this.adaptiveCardService.buildErrorCard('❌ Kunne ikke oppdatere samtaler.');
      }
    });

    // Cancel actions
    this.app.adaptiveCards.actionExecute('cancelRename', async (context, state, data) => {
      return '❌ Navneendring avbrutt.';
    });

    this.app.adaptiveCards.actionExecute('cancelAction', async (context, state, data) => {
      return '❌ Handling avbrutt.';
    });

    this.app.adaptiveCards.actionExecute('dismissSuccess', async (context, state, data) => {
      return 'OK';
    });

    this.app.adaptiveCards.actionExecute('dismissError', async (context, state, data) => {
      return 'OK';
    });

    this.logger.log('✅ Session management commands registered');
  }

  /**
   * Process Teams file attachments
   */
  private async processTeamsAttachments(attachments: any[], context: any): Promise<any[]> {
    const processedAttachments = [];
    
    for (const attachment of attachments) {
      try {
        this.logger.log(`📎 Processing attachment: ${attachment.name} (${attachment.contentType})`);
        
        const attachmentData = {
          id: `teams-${Date.now()}-${Math.random()}`,
          originalName: attachment.name,
          mimeType: attachment.contentType,
          size: 0, // Would be actual size from download
          buffer: Buffer.alloc(0), // Would be actual file content
          uploadedBy: context.activity.from.aadObjectId || context.activity.from.id,
          tenantId: context.activity.channelData?.tenant?.id,
          metadata: {
            source: 'teams',
            contentUrl: attachment.contentUrl,
            thumbnailUrl: attachment.thumbnailUrl
          }
        };
        
        processedAttachments.push(attachmentData);
        
      } catch (error) {
        this.logger.error(`❌ Failed to process attachment ${attachment.name}:`, error);
      }
    }
    
    return processedAttachments;
  }

  /**
   * Process incoming Bot Framework activity
   */
  async processActivity(req: Request, res: Response): Promise<void> {
    // Add early diagnostics for 401 troubleshooting
    try {
      const body: any = (req as any).body || {};
      const channelId = body.channelId;
      const serviceUrl = body.serviceUrl;
      const authHeaderPresent = !!req.headers['authorization'];
      this.logger.log(`🛰️ Incoming activity diagnostics: channelId=${channelId || 'N/A'} serviceUrl=${serviceUrl || 'N/A'} authHeader=${authHeaderPresent}`);
    } catch (e) {
      this.logger.warn(`⚠️ Failed to extract preliminary activity diagnostics: ${(e as Error).message}`);
    }
    return this.adapter.process(req, res, async (context) => {
      await this.app.run(context);
    });
  }

  /**
   * Get the underlying adapter for advanced operations
   */
  getAdapter(): TeamsAdapter {
    return this.adapter;
  }

  /**
   * Get the Teams AI application instance
   */
  getApplication(): Application<TurnState> {
    return this.app;
  }
}
