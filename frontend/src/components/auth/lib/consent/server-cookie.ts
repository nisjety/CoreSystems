/**
 * Server-side Consent Cookie Management
 * Implements secure, httpOnly + secure cookies for consent with version, timestamp, and purposes
 */

import type {
  ConsentPurposes,
  ServerConsentCookie,
  ConsentAuditLog,
  ConsentMethod,
  ConsentPurpose
} from './types';

// Re-export types for backward compatibility
export type {
  ConsentPurposes,
  ServerConsentCookie,
  ConsentAuditLog,
  ConsentMethod,
  ConsentPurpose
};

/**
 * Server Actions for setting and reading consent
 */
export class ConsentServerActions {
  private static readonly COOKIE_NAME = 'user_consent';
  private static readonly CURRENT_VERSION = '2.2.0';
  private static readonly DEFAULT_TTL = 365 * 24 * 60 * 60; // 1 year in seconds

  /**
   * Set consent cookie on server
   */
  static async setConsent(
    purposes: ConsentPurposes,
    options: {
      userId?: string;
      sessionId: string;
      userAgent: string;
      ipAddress: string;
      geoLocation?: string;
      tcString?: string;
      additionalConsent?: string;
      ttl?: number;
    }
  ): Promise<{ success: boolean; consentId: string; auditId: string }> {
    const consentId = this.generateConsentId();
    const timestamp = Date.now();
    const ttl = options.ttl || this.DEFAULT_TTL;

    // Create consent cookie data
    const consentData: ServerConsentCookie = {
      version: this.CURRENT_VERSION,
      timestamp,
      purposes,
      consentId,
      userAgent: options.userAgent,
      ipHash: this.hashIP(options.ipAddress),
      geoLocation: options.geoLocation,
      tcString: options.tcString,
      additionalConsent: options.additionalConsent,
    };

    // Create audit log entry
    const auditEntry: ConsentAuditLog = {
      id: this.generateAuditId(),
      userId: options.userId,
      sessionId: options.sessionId,
      purposes,
      version: this.CURRENT_VERSION,
      timestamp,
      action: 'granted',
      method: 'api',
      userAgent: options.userAgent,
      ipAddress: options.ipAddress,
      geoLocation: options.geoLocation,
      tcString: options.tcString,
      additionalConsent: options.additionalConsent,
      ttl,
    };

    try {
      // Set httpOnly + secure cookie
      await this.setCookie(consentData, ttl);
      
      // Log to audit database
      const auditId = await this.logToAudit(auditEntry);
      
      // Dispatch custom event for real-time updates
      this.dispatchConsentEvent('consent-updated', {
        consentId,
        purposes,
        timestamp,
      });

      return { success: true, consentId, auditId };
    } catch (error) {
      console.error('Failed to set consent:', error);
      return { success: false, consentId: '', auditId: '' };
    }
  }

  /**
   * Read consent from server cookie
   */
  static async getConsent(
    request: Request
  ): Promise<ServerConsentCookie | null> {
    try {
      const cookieHeader = request.headers.get('cookie');
      if (!cookieHeader) return null;

      const cookies = this.parseCookies(cookieHeader);
      const consentCookie = cookies[this.COOKIE_NAME];
      
      if (!consentCookie) return null;

      const consentData = JSON.parse(
        Buffer.from(consentCookie, 'base64').toString('utf-8')
      ) as ServerConsentCookie;

      // Validate consent version and expiry
      if (!this.isValidConsent(consentData)) {
        return null;
      }

      return consentData;
    } catch (error) {
      console.error('Failed to read consent:', error);
      return null;
    }
  }

  /**
   * Update existing consent
   */
  static async updateConsent(
    request: Request,
    newPurposes: Partial<ConsentPurposes>,
    options: {
      userId?: string;
      sessionId: string;
      userAgent: string;
      ipAddress: string;
      method: ConsentMethod;
    }
  ): Promise<{ success: boolean; consentId: string }> {
    const existingConsent = await this.getConsent(request);
    if (!existingConsent) {
      throw new Error('No existing consent found');
    }

    const updatedPurposes: ConsentPurposes = {
      ...existingConsent.purposes,
      ...newPurposes,
    };

    const result = await this.setConsent(updatedPurposes, {
      ...options,
      ttl: this.DEFAULT_TTL,
    });

    // Log update action
    if (result.success) {
      await this.logToAudit({
        id: this.generateAuditId(),
        userId: options.userId,
        sessionId: options.sessionId,
        purposes: updatedPurposes,
        version: this.CURRENT_VERSION,
        timestamp: Date.now(),
        action: 'updated',
        method: options.method,
        userAgent: options.userAgent,
        ipAddress: options.ipAddress,
        ttl: this.DEFAULT_TTL,
      });
    }

    return result;
  }

  /**
   * Withdraw consent
   */
  static async withdrawConsent(
    request: Request,
    options: {
      userId?: string;
      sessionId: string;
      userAgent: string;
      ipAddress: string;
      method: 'user' | 'api' | 'gdpr_request';
    }
  ): Promise<{ success: boolean }> {
    const existingConsent = await this.getConsent(request);
    if (!existingConsent) {
      return { success: false };
    }

    try {
      // Clear the consent cookie
      await this.clearCookie();

      // Log withdrawal
      await this.logToAudit({
        id: this.generateAuditId(),
        userId: options.userId,
        sessionId: options.sessionId,
        purposes: existingConsent.purposes,
        version: this.CURRENT_VERSION,
        timestamp: Date.now(),
        action: 'withdrawn',
        method: options.method,
        userAgent: options.userAgent,
        ipAddress: options.ipAddress,
        ttl: 0,
      });

      this.dispatchConsentEvent('consent-withdrawn', {
        consentId: existingConsent.consentId,
        timestamp: Date.now(),
      });

      return { success: true };
    } catch (error) {
      console.error('Failed to withdraw consent:', error);
      return { success: false };
    }
  }

  /**
   * Get consent audit logs for a user
   */
  static async getAuditLogs(
     
    _userId: string
    // Disabled options parameter for now - would be used for pagination/filtering
    // options: { limit?: number; offset?: number; startDate?: Date; endDate?: Date; } = {}
  ): Promise<ConsentAuditLog[]> {
    // Implementation would connect to your audit database
    // This is a placeholder for the actual database query
    try {
      // Implementation would connect to your audit database
      // This is a placeholder for the actual database query
      // const query = this.buildAuditQuery(userId, options);
      // const logs = await database.query(query);
      // return logs;
      return []; // Placeholder for database results
    } catch (error) {
      console.error('Failed to get audit logs:', error);
      return [];
    }
  }

  /**
   * Check if specific purpose is consented
   */
  static checkPurposeConsent(
    consent: ServerConsentCookie | null,
    purpose: ConsentPurpose
  ): boolean {
    if (!consent || !this.isValidConsent(consent)) {
      return false;
    }
    return consent.purposes[purpose] === true;
  }

  /**
   * Private helper methods
   */
  private static async setCookie(
    data: ServerConsentCookie,
    ttl: number
  ): Promise<void> {
    // Implementation would be in your server framework (Next.js, Express, etc.)
    const encodedData = Buffer.from(JSON.stringify(data)).toString('base64');
    const expires = new Date(Date.now() + ttl * 1000);
    
    // For Next.js API routes:
    // response.setHeader('Set-Cookie', 
    //   `${this.COOKIE_NAME}=${encodedData}; HttpOnly; Secure; SameSite=Strict; Expires=${expires.toUTCString()}; Path=/`
    // );
    
    // Placeholder implementation - in real app, this would set the actual cookie
    console.log(`Setting consent cookie expires: ${expires.toISOString()}`);
    console.log(`Cookie data: ${encodedData.substring(0, 50)}...`);
  }

  private static async clearCookie(): Promise<void> {
    // Clear the consent cookie
    // response.setHeader('Set-Cookie', 
    //   `${this.COOKIE_NAME}=; HttpOnly; Secure; SameSite=Strict; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/`
    // );
  }

  private static parseCookies(cookieHeader: string): Record<string, string> {
    return cookieHeader
      .split(';')
      .map(cookie => cookie.trim().split('='))
      .reduce((acc, [name, value]) => {
        if (name && value) {
          acc[name] = decodeURIComponent(value);
        }
        return acc;
      }, {} as Record<string, string>);
  }

  private static isValidConsent(consent: ServerConsentCookie): boolean {
    const now = Date.now();
    const oneYear = 365 * 24 * 60 * 60 * 1000;
    
    // Check if consent is not older than 1 year
    if (now - consent.timestamp > oneYear) {
      return false;
    }

    // Check if version is compatible
    if (!consent.version || consent.version < '2.0.0') {
      return false;
    }

    return true;
  }

  private static async logToAudit(entry: ConsentAuditLog): Promise<string> {
    // Implementation would save to your audit database
    // This is a placeholder for the actual database insert
    try {
      // await database.insert('consent_audit', entry);
      return entry.id;
    } catch (error) {
      console.error('Failed to log audit entry:', error);
      throw error;
    }
  }

  private static generateConsentId(): string {
    return `consent_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  private static generateAuditId(): string {
    return `audit_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
  }

  private static hashIP(ip: string): string {
    // Simple hash for IP anonymization
    // In production, use a proper hashing algorithm like SHA-256
    let hash = 0;
    for (let i = 0; i < ip.length; i++) {
      const char = ip.charCodeAt(i);
      hash = ((hash << 5) - hash) + char;
      hash = hash & hash; // Convert to 32bit integer
    }
    return hash.toString(36);
  }

  private static buildAuditQuery(
    userId: string,
    options: {
      limit?: number;
      offset?: number;
      startDate?: Date;
      endDate?: Date;
    }
  ): string {
    // Build SQL query for audit logs
    let query = `SELECT * FROM consent_audit WHERE userId = ?`;
    const params = [userId];

    if (options.startDate) {
      query += ` AND timestamp >= ?`;
      params.push(options.startDate.getTime().toString());
    }

    if (options.endDate) {
      query += ` AND timestamp <= ?`;
      params.push(options.endDate.getTime().toString());
    }

    query += ` ORDER BY timestamp DESC`;

    if (options.limit) {
      query += ` LIMIT ?`;
      params.push(options.limit.toString());
    }

    if (options.offset) {
      query += ` OFFSET ?`;
      params.push(options.offset.toString());
    }

    return query;
  }

  private static dispatchConsentEvent(
    eventType: string,
    data: Record<string, unknown>
  ): void {
    if (typeof window !== 'undefined') {
      window.dispatchEvent(
        new CustomEvent(eventType, { detail: data })
      );
    }
  }
}

/**
 * Middleware helper for checking consent before processing
 */
export class ConsentMiddleware {
  static async checkPurposeConsent(
    request: Request,
    requiredPurpose: ConsentPurpose
  ): Promise<boolean> {
    const consent = await ConsentServerActions.getConsent(request);
    return ConsentServerActions.checkPurposeConsent(consent, requiredPurpose);
  }

  static async requireConsent(
    request: Request,
    requiredPurposes: ConsentPurpose[]
  ): Promise<{ allowed: boolean; missingPurposes: string[] }> {
    const consent = await ConsentServerActions.getConsent(request);
    const missingPurposes: string[] = [];

    for (const purpose of requiredPurposes) {
      if (!ConsentServerActions.checkPurposeConsent(consent, purpose)) {
        missingPurposes.push(purpose);
      }
    }

    return {
      allowed: missingPurposes.length === 0,
      missingPurposes,
    };
  }
}

export default ConsentServerActions;
