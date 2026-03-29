/**
 * Consent Middleware for Blocking Third-Party Scripts and Resources
 * Implements Purpose-based blocking with Permissions-Policy and Sec-CH-UA headers
 */

import { NextRequest, NextResponse } from 'next/server';
import { ConsentServerActions } from './server-cookie';
import type { ConsentPurposes } from './types';

export interface ScriptBlocking {
  purpose: keyof ConsentPurposes;
  domains: string[];
  scripts: string[];
  resources: string[];
  alternativeUrl?: string; // "no-tracking" variant
}

export interface SecurityHeaders {
  'Permissions-Policy': string;
  'Sec-CH-UA': string;
  'Sec-CH-UA-Mobile': string;
  'Sec-CH-UA-Platform': string;
  'Cross-Origin-Embedder-Policy': string;
  'Cross-Origin-Opener-Policy': string;
  'Cross-Origin-Resource-Policy': string;
  'Referrer-Policy': string;
  'X-Content-Type-Options': string;
  'X-Frame-Options': string;
  'Content-Security-Policy': string;
}

/**
 * Configuration for different types of blocked content
 */
const BLOCKED_CONTENT: ScriptBlocking[] = [
  {
    purpose: 'analytics',
    domains: [
      'google-analytics.com',
      'googletagmanager.com',
      'analytics.google.com',
      'doubleclick.net',
    ],
    scripts: [
      '/gtag/js',
      '/analytics.js',
      '/gtm.js',
    ],
    resources: [
      'google-analytics.com/collect',
      'google-analytics.com/g/collect',
    ],
    alternativeUrl: '/api/analytics/no-track',
  },
  {
    purpose: 'ads',
    domains: [
      'googlesyndication.com',
      'googleadservices.com',
      'doubleclick.net',
      'adsystem.amazon.com',
    ],
    scripts: [
      '/adsbygoogle.js',
      '/ads/',
      '/tr',
    ],
    resources: [
      'googlesyndication.com/pagead',
      'amazon-adsystem.com/aax2',
    ],
    alternativeUrl: '/api/ads/no-track',
  },
  {
    purpose: 'ab_test',
    domains: [
      'optimizely.com',
      'googleoptimize.com',
      'vwo.com',
      'split.io',
    ],
    scripts: [
      '/optimize.js',
      '/ab-test.js',
    ],
    resources: [],
    alternativeUrl: '/api/ab-test/no-track',
  },
  {
    purpose: 'heatmap',
    domains: [
      'hotjar.com',
      'mouseflow.com',
      'crazyegg.com',
      'fullstory.com',
    ],
    scripts: [
      '/heatmap.js',
      '/recording.js',
    ],
    resources: [],
    alternativeUrl: '/api/heatmap/no-track',
  },
  {
    purpose: 'functional',
    domains: [
      'intercom.io',
      'zendesk.com',
      'typeform.com',
      'stripe.com',
    ],
    scripts: [
      '/widget.js',
      '/chat.js',
      '/stripe.js',
    ],
    resources: [],
  },
];

/**
 * Consent-aware middleware for Next.js
 */
export async function consentMiddleware(request: NextRequest): Promise<NextResponse> {
  const response = NextResponse.next();
  
  // Get user consent from server cookie
  const consent = await ConsentServerActions.getConsent(request);
  
  // Apply security headers based on consent
  applySecurityHeaders(response, consent?.purposes);
  
  // Check if this request should be blocked
  const shouldBlock = await shouldBlockRequest(request, consent?.purposes);
  
  if (shouldBlock.blocked) {
    return handleBlockedRequest(request, shouldBlock);
  }
  
  // Add consent information to response headers for frontend
  if (consent) {
    response.headers.set('X-Consent-Purposes', JSON.stringify(consent.purposes));
    response.headers.set('X-Consent-Version', consent.version);
    response.headers.set('X-Consent-Timestamp', consent.timestamp.toString());
  }
  
  return response;
}

/**
 * Check if a request should be blocked based on consent
 */
async function shouldBlockRequest(
  request: NextRequest,
  purposes?: ConsentPurposes
): Promise<{
  blocked: boolean;
  reason?: string;
  purpose?: keyof ConsentPurposes;
  alternative?: string;
}> {
  const url = request.url;
  const pathname = new URL(url).pathname;
  const hostname = new URL(url).hostname;
  
  // If no consent, block everything except necessary
  if (!purposes) {
    const blockConfig = findBlockingConfig(hostname, pathname);
    if (blockConfig && blockConfig.purpose !== 'necessary') {
      return {
        blocked: true,
        reason: 'No consent provided',
        purpose: blockConfig.purpose,
        alternative: blockConfig.alternativeUrl,
      };
    }
    return { blocked: false };
  }
  
  // Check each blocking configuration
  for (const config of BLOCKED_CONTENT) {
    if (isMatchingRequest(hostname, pathname, config)) {
      if (!purposes[config.purpose]) {
        return {
          blocked: true,
          reason: `Purpose '${config.purpose}' not consented`,
          purpose: config.purpose,
          alternative: config.alternativeUrl,
        };
      }
    }
  }
  
  return { blocked: false };
}

/**
 * Check if request matches blocking configuration
 */
function isMatchingRequest(
  hostname: string,
  pathname: string,
  config: ScriptBlocking
): boolean {
  // Check domains
  const isDomainMatch = config.domains.some(domain => 
    hostname.includes(domain) || hostname.endsWith(domain)
  );
  
  // Check script paths
  const isScriptMatch = config.scripts.some(script => 
    pathname.includes(script)
  );
  
  // Check resource paths
  const isResourceMatch = config.resources.some(resource => 
    `${hostname}${pathname}`.includes(resource)
  );
  
  return isDomainMatch || isScriptMatch || isResourceMatch;
}

/**
 * Find blocking configuration for a request
 */
function findBlockingConfig(
  hostname: string,
  pathname: string
): ScriptBlocking | undefined {
  return BLOCKED_CONTENT.find(config => 
    isMatchingRequest(hostname, pathname, config)
  );
}

/**
 * Handle blocked request - redirect to alternative or return empty response
 */
function handleBlockedRequest(
  request: NextRequest,
  blockInfo: { blocked: boolean; reason?: string; purpose?: keyof ConsentPurposes; alternative?: string }
): NextResponse {
  // If there's an alternative URL (no-tracking variant), redirect
  if (blockInfo.alternative) {
    const alternativeUrl = new URL(blockInfo.alternative, request.url);
    return NextResponse.redirect(alternativeUrl);
  }
  
  // For JavaScript files, return empty script
  if (request.url.endsWith('.js')) {
    return new NextResponse(
      `// Script blocked: ${blockInfo.reason}\nconsole.log('Script blocked due to consent: ${blockInfo.purpose}');`,
      {
        status: 200,
        headers: {
          'Content-Type': 'application/javascript',
          'X-Blocked-Reason': blockInfo.reason || 'Consent required',
          'X-Blocked-Purpose': blockInfo.purpose || 'unknown',
        },
      }
    );
  }
  
  // For other resources, return 204 No Content
  return new NextResponse(null, {
    status: 204,
    headers: {
      'X-Blocked-Reason': blockInfo.reason || 'Consent required',
      'X-Blocked-Purpose': blockInfo.purpose || 'unknown',
    },
  });
}

/**
 * Apply security headers based on consent
 */
function applySecurityHeaders(
  response: NextResponse,
  purposes?: ConsentPurposes
): void {
  const headers: Partial<SecurityHeaders> = {
    'X-Content-Type-Options': 'nosniff',
    'X-Frame-Options': 'DENY',
    'Referrer-Policy': 'strict-origin-when-cross-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Resource-Policy': 'same-origin',
  };
  
  // Permissions Policy based on consent
  const permissions: string[] = [];
  
  if (!purposes?.analytics) {
    permissions.push('browsing-topics=()');
    permissions.push('attribution-reporting=()');
  }
  
  if (!purposes?.ads) {
    permissions.push('interest-cohort=()');
    permissions.push('attribution-reporting=()');
  }
  
  if (!purposes?.heatmap) {
    permissions.push('screen-wake-lock=()');
    permissions.push('display-capture=()');
  }
  
  if (!purposes?.functional) {
    permissions.push('payment=()');
    permissions.push('geolocation=()');
    permissions.push('camera=()');
    permissions.push('microphone=()');
  }
  
  headers['Permissions-Policy'] = permissions.join(', ');
  
  // User Agent Client Hints based on consent
  if (!purposes?.analytics && !purposes?.ads) {
    headers['Sec-CH-UA'] = '"Not)A;Brand";v="99"';
    headers['Sec-CH-UA-Mobile'] = '?0';
    headers['Sec-CH-UA-Platform'] = '"Unknown"';
  }
  
  // Content Security Policy
  const cspDirectives: string[] = [
    "default-src 'self'",
    "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data: blob:",
    "font-src 'self' data:",
    "connect-src 'self'",
    "frame-ancestors 'none'",
    "base-uri 'self'",
    "form-action 'self'",
  ];
  
  // Add external domains based on consent
  if (purposes?.analytics) {
    cspDirectives.push("script-src 'self' 'unsafe-inline' *.google-analytics.com *.googletagmanager.com");
    cspDirectives.push("connect-src 'self' *.google-analytics.com *.analytics.google.com");
  }
  
  if (purposes?.ads) {
    cspDirectives.push("script-src 'self' 'unsafe-inline' *.googlesyndication.com");
    cspDirectives.push("connect-src 'self' *.googlesyndication.com");
  }
  
  if (purposes?.functional) {
    cspDirectives.push("script-src 'self' 'unsafe-inline' *.stripe.com *.intercom.io");
    cspDirectives.push("connect-src 'self' *.stripe.com *.intercom.io");
  }
  
  headers['Content-Security-Policy'] = cspDirectives.join('; ');
  
  // Apply all headers
  Object.entries(headers).forEach(([name, value]) => {
    if (value) {
      response.headers.set(name, value);
    }
  });
}

/**
 * Helper function for API routes to check consent
 */
export async function requireConsentForAPI(
  request: NextRequest,
  requiredPurpose: keyof ConsentPurposes
): Promise<NextResponse | null> {
  const consent = await ConsentServerActions.getConsent(request);
  
  if (!consent || !consent.purposes[requiredPurpose]) {
    return NextResponse.json(
      {
        error: 'Consent required',
        purpose: requiredPurpose,
        message: `This endpoint requires consent for '${requiredPurpose}' purpose.`,
      },
      { 
        status: 403,
        headers: {
          'X-Consent-Required': requiredPurpose,
          'X-Consent-Status': consent ? 'partial' : 'missing',
        },
      }
    );
  }
  
  return null; // Allow the request to proceed
}

/**
 * Express.js middleware equivalent
 */
export function createExpressConsentMiddleware() {
   
  return async (req: any, res: any, next: any) => {
    try {
      // Convert Express request to NextRequest-like object for consent checking
      const url = `${req.protocol}://${req.get('host')}${req.originalUrl}`;
      
      // Create a mock NextRequest for consent checking
      const mockRequest = {
        url,
        method: req.method,
        headers: new Headers(req.headers),
        nextUrl: new URL(url),
      } as NextRequest;
      
      const consent = await ConsentServerActions.getConsent(mockRequest);
      const shouldBlock = await shouldBlockRequest(mockRequest, consent?.purposes);
      
      if (shouldBlock.blocked) {
        if (shouldBlock.alternative) {
          return res.redirect(302, shouldBlock.alternative);
        }
        
        if (req.originalUrl.endsWith('.js')) {
          res.set('Content-Type', 'application/javascript');
          return res.send(`// Script blocked: ${shouldBlock.reason}`);
        }
        
        return res.status(204).end();
      }
      
      // Add consent headers
      if (consent) {
        res.set('X-Consent-Purposes', JSON.stringify(consent.purposes));
        res.set('X-Consent-Version', consent.version);
      }
      
      next();
    } catch (error) {
      console.error('Consent middleware error:', error);
      next(); // Continue on error
    }
  };
}

export default consentMiddleware;
