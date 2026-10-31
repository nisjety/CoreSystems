import { Injectable } from '@nestjs/common';
import { SessionSummary } from './session.service';
import { ThreadSwitcherData } from './session-command.service';

export interface AdaptiveCardData {
  type: "AdaptiveCard";
  version: string;
  body: any[];
  actions?: any[];
}

@Injectable()
export class AdaptiveCardService {
  
  /**
   * Build Thread Switcher Adaptive Card
   */
  buildThreadSwitcherCard(data: ThreadSwitcherData): AdaptiveCardData {
    const { sessions, activeSessionId, totalCount } = data;

    // Build session items
    const sessionItems = sessions.map(session => this.buildSessionItem(session, session.id === activeSessionId));

    // Build main card
    const card: AdaptiveCardData = {
      type: "AdaptiveCard",
      version: "1.5",
      body: [
        {
          type: "TextBlock",
          text: "📋 Dine samtaler",
          weight: "Bolder",
          size: "Medium",
          color: "Accent"
        },
        {
          type: "TextBlock", 
          text: totalCount > 0 
            ? `${totalCount} aktive samtaler funnet:`
            : "Ingen aktive samtaler funnet.",
          wrap: true,
          spacing: "Small"
        }
      ]
    };

    // Add sessions container if we have sessions
    if (sessions.length > 0) {
      card.body.push({
        type: "Container",
        spacing: "Medium",
        items: [
          {
            type: "ColumnSet",
            columns: [
              {
                type: "Column",
                width: "auto",
                items: [
                  {
                    type: "TextBlock",
                    text: "💬",
                    size: "Small"
                  }
                ]
              },
              {
                type: "Column", 
                width: "stretch",
                items: [
                  {
                    type: "TextBlock",
                    text: "**Titel**",
                    weight: "Bolder",
                    size: "Small"
                  }
                ]
              },
              {
                type: "Column",
                width: "auto", 
                items: [
                  {
                    type: "TextBlock",
                    text: "**Meldinger**",
                    weight: "Bolder",
                    size: "Small"
                  }
                ]
              },
              {
                type: "Column",
                width: "auto",
                items: [
                  {
                    type: "TextBlock",
                    text: "**Handling**",
                    weight: "Bolder", 
                    size: "Small"
                  }
                ]
              }
            ]
          },
          // Add separator
          {
            type: "TextBlock",
            text: "─".repeat(50),
            color: "Attention",
            spacing: "Small"
          },
          // Add sessions
          ...sessionItems
        ]
      });

      // Add action buttons at bottom
      card.actions = [
        {
          type: "Action.Execute",
          title: "🆕 Ny samtale",
          verb: "newSession",
          data: {}
        },
        {
          type: "Action.Execute", 
          title: "🔄 Oppdater",
          verb: "refreshSessions",
          data: {}
        }
      ];
    } else {
      // No sessions - just show create new button
      card.actions = [
        {
          type: "Action.Execute",
          title: "🆕 Opprett første samtale",
          verb: "newSession", 
          data: {}
        }
      ];
    }

    return card;
  }

  /**
   * Build individual session item
   */
  private buildSessionItem(session: SessionSummary, isActive: boolean): any {
    const statusIcon = isActive ? "🟢" : (session.isPinned ? "📌" : "💬");
    const titleColor = isActive ? "Good" : "Default";
    const titleWeight = isActive ? "Bolder" : "Default";
    
    // Enhanced title display with AI indicator
    const isAiGenerated = !session.title.startsWith('Ny samtale') && !session.title.startsWith('Samtale ');
    const titlePrefix = isAiGenerated ? "🤖 " : "";

    return {
      type: "ColumnSet",
      spacing: "Small",
      style: isActive ? "emphasis" : "default",
      columns: [
        {
          type: "Column",
          width: "auto",
          items: [
            {
              type: "TextBlock",
              text: statusIcon,
              size: "Small"
            }
          ]
        },
        {
          type: "Column", 
          width: "stretch",
          items: [
            {
              type: "TextBlock",
              text: titlePrefix + this.truncateText(session.title, 25),
              weight: titleWeight,
              color: titleColor,
              wrap: false
            },
            {
              type: "TextBlock",
              text: this.formatSessionSubtitle(session),
              size: "Small",
              color: "Attention", 
              spacing: "None"
            }
          ]
        },
        {
          type: "Column",
          width: "auto",
          items: [
            {
              type: "TextBlock",
              text: session.messageCount.toString(),
              horizontalAlignment: "Center",
              color: "Attention"
            }
          ]
        },
        {
          type: "Column",
          width: "auto",
          items: [
            {
              type: "ActionSet",
              actions: this.buildSessionActions(session, isActive)
            }
          ]
        }
      ]
    };
  }

  /**
   * Build action buttons for a session
   */
  private buildSessionActions(session: SessionSummary, isActive: boolean): any[] {
    const actions = [];

    if (!isActive) {
      actions.push({
        type: "Action.Execute",
        title: "↩️",
        tooltip: "Bytt til denne samtalen",
        verb: "switchSession",
        data: { sessionId: session.id }
      });
    }

    actions.push({
      type: "Action.Execute", 
      title: "✏️",
      tooltip: "Endre navn",
      verb: "renameSession",
      data: { sessionId: session.id, currentTitle: session.title }
    });

    // Add AI title generation action
    actions.push({
      type: "Action.Execute",
      title: "🤖",
      tooltip: "Generer AI-tittel",
      verb: "generateAiTitle",
      data: { sessionId: session.id }
    });

    if (session.isPinned) {
      actions.push({
        type: "Action.Execute",
        title: "📌",
        tooltip: "Løsne fra toppen", 
        verb: "unpinSession",
        data: { sessionId: session.id }
      });
    } else {
      actions.push({
        type: "Action.Execute",
        title: "📎",
        tooltip: "Fest øverst",
        verb: "pinSession", 
        data: { sessionId: session.id }
      });
    }

    actions.push({
      type: "Action.Execute",
      title: "📦",
      tooltip: "Arkiver samtale",
      verb: "archiveSession",
      data: { sessionId: session.id }
    });

    return actions;
  }

  /**
   * Build rename session card
   */
  buildRenameSessionCard(sessionId: string, currentTitle: string): AdaptiveCardData {
    return {
      type: "AdaptiveCard",
      version: "1.5", 
      body: [
        {
          type: "TextBlock",
          text: "✏️ Endre samtalenavn",
          weight: "Bolder",
          size: "Medium"
        },
        {
          type: "TextBlock",
          text: "Skriv inn det nye navnet for samtalen:",
          wrap: true,
          spacing: "Small"
        },
        {
          type: "Input.Text",
          id: "newTitle",
          value: currentTitle,
          placeholder: "Skriv nytt navn her...",
          maxLength: 100
        }
      ],
      actions: [
        {
          type: "Action.Execute",
          title: "💾 Lagre",
          verb: "saveRename",
          data: { sessionId }
        },
        {
          type: "Action.Execute", 
          title: "❌ Avbryt",
          verb: "cancelRename",
          data: {}
        }
      ]
    };
  }

  /**
   * Build confirmation card for destructive actions
   */
  buildConfirmationCard(action: string, sessionId: string, sessionTitle: string): AdaptiveCardData {
    const actionData = this.getActionData(action);

    return {
      type: "AdaptiveCard",
      version: "1.5",
      body: [
        {
          type: "TextBlock", 
          text: `${actionData.icon} ${actionData.title}`,
          weight: "Bolder",
          size: "Medium",
          color: actionData.color
        },
        {
          type: "TextBlock",
          text: `Er du sikker på at du vil ${actionData.verb} samtalen "${sessionTitle}"?`,
          wrap: true,
          spacing: "Small"
        },
        {
          type: "TextBlock",
          text: actionData.warning,
          wrap: true, 
          color: "Warning",
          spacing: "Small"
        }
      ],
      actions: [
        {
          type: "Action.Execute",
          title: `${actionData.icon} Ja, ${actionData.verb}`,
          verb: `confirm_${action}`,
          data: { sessionId },
          style: "destructive"
        },
        {
          type: "Action.Execute",
          title: "❌ Avbryt", 
          verb: "cancelAction",
          data: {}
        }
      ]
    };
  }

  /**
   * Build success notification card
   */
  buildSuccessCard(message: string, autoClose = true): AdaptiveCardData {
    const card: AdaptiveCardData = {
      type: "AdaptiveCard",
      version: "1.5",
      body: [
        {
          type: "ColumnSet",
          columns: [
            {
              type: "Column",
              width: "auto", 
              items: [
                {
                  type: "TextBlock",
                  text: "✅",
                  size: "Medium"
                }
              ]
            },
            {
              type: "Column",
              width: "stretch",
              items: [
                {
                  type: "TextBlock",
                  text: message,
                  weight: "Bolder",
                  color: "Good",
                  wrap: true
                }
              ]
            }
          ]
        }
      ]
    };

    if (!autoClose) {
      card.actions = [
        {
          type: "Action.Execute",
          title: "OK",
          verb: "dismissSuccess",
          data: {}
        }
      ];
    }

    return card;
  }

  /**
   * Build error notification card 
   */
  buildErrorCard(message: string): AdaptiveCardData {
    return {
      type: "AdaptiveCard",
      version: "1.5",
      body: [
        {
          type: "ColumnSet", 
          columns: [
            {
              type: "Column",
              width: "auto",
              items: [
                {
                  type: "TextBlock",
                  text: "❌",
                  size: "Medium"
                }
              ]
            },
            {
              type: "Column",
              width: "stretch", 
              items: [
                {
                  type: "TextBlock",
                  text: message,
                  weight: "Bolder",
                  color: "Attention",
                  wrap: true
                }
              ]
            }
          ]
        }
      ],
      actions: [
        {
          type: "Action.Execute",
          title: "OK",
          verb: "dismissError", 
          data: {}
        }
      ]
    };
  }

  /**
   * Utility methods
   */
  private truncateText(text: string, maxLength: number): string {
    if (text.length <= maxLength) return text;
    return text.substring(0, maxLength - 3) + '...';
  }

  private formatSessionSubtitle(session: SessionSummary): string {
    const timeDiff = Date.now() - session.lastMessageAt.getTime();
    const minutes = Math.floor(timeDiff / (1000 * 60));
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    let timeText = '';
    if (days > 0) {
      timeText = `${days}d siden`;
    } else if (hours > 0) {
      timeText = `${hours}t siden`;
    } else if (minutes > 0) {
      timeText = `${minutes}m siden`;
    } else {
      timeText = 'Nå';
    }

    return `ID: ${session.id.substring(0, 8)} • ${timeText}`;
  }

  private getActionData(action: string): any {
    const actions = {
      archive: {
        icon: '📦',
        title: 'Arkiver samtale',
        verb: 'arkivere',
        color: 'Warning',
        warning: 'Samtalen vil bli flyttet til arkivet og kan gjenopprettes senere.'
      },
      delete: {
        icon: '🗑️', 
        title: 'Slett samtale',
        verb: 'slette',
        color: 'Attention',
        warning: 'Denne handlingen kan ikke angres. All meldingshistorikk vil bli permanent slettet.'
      }
    };

    return actions[action] || {
      icon: '⚠️',
      title: 'Ukjent handling',
      verb: 'utføre',
      color: 'Attention', 
      warning: 'Dette er en ukjent handling.'
    };
  }
}
