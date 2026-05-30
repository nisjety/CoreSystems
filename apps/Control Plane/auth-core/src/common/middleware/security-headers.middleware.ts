import { Injectable, NestMiddleware } from '@nestjs/common';
import { Request, Response, NextFunction } from 'express';

@Injectable()
export class SecurityHeadersMiddleware implements NestMiddleware {
  use(req: Request, res: Response, next: NextFunction): void {
    // Content Security Policy (CSP) - Strict policy for authentication service
    const csp = [
      "default-src 'self'",
      "script-src 'self' 'unsafe-inline' 'unsafe-eval'", // Needed for Better Auth client
      "style-src 'self' 'unsafe-inline'", // Allow inline styles for UI components
      "img-src 'self' data: https:", // Allow images from self, data URLs, and HTTPS
      "connect-src 'self' https://graph.microsoft.com https://login.microsoftonline.com", // API connections
      "font-src 'self' data:",
      "object-src 'none'", // Block object/embed/applet
      "base-uri 'self'", // Restrict base URI
      "form-action 'self'", // Restrict form submissions
      "frame-ancestors 'none'", // Prevent framing (equivalent to X-Frame-Options: DENY)
      'upgrade-insecure-requests', // Force HTTPS
      process.env.NODE_ENV === 'development' ? 'block-all-mixed-content' : '',
      process.env.CSP_REPORT_URI
        ? `report-uri ${process.env.CSP_REPORT_URI}`
        : '',
    ]
      .filter(Boolean)
      .join('; ');

    res.setHeader('Content-Security-Policy', csp);

    // HTTP Strict Transport Security (HSTS)
    // Tell browsers to only use HTTPS for this domain for the next year
    if (process.env.NODE_ENV !== 'development') {
      res.setHeader(
        'Strict-Transport-Security',
        'max-age=31536000; includeSubDomains; preload',
      );
    }

    // X-Frame-Options - Prevent clickjacking
    res.setHeader('X-Frame-Options', 'DENY');

    // X-Content-Type-Options - Prevent MIME type sniffing
    res.setHeader('X-Content-Type-Options', 'nosniff');

    // Referrer Policy - Control referrer information
    res.setHeader('Referrer-Policy', 'strict-origin-when-cross-origin');

    // X-XSS-Protection - Enable XSS filtering (legacy but still useful)
    res.setHeader('X-XSS-Protection', '1; mode=block');

    // X-Permitted-Cross-Domain-Policies - Restrict cross-domain access
    res.setHeader('X-Permitted-Cross-Domain-Policies', 'none');

    // X-DNS-Prefetch-Control - Control DNS prefetching
    res.setHeader('X-DNS-Prefetch-Control', 'off');

    // Permissions Policy (formerly Feature Policy) - Control browser features
    const permissionsPolicy = [
      'camera=()', // Disable camera
      'microphone=()', // Disable microphone
      'geolocation=()', // Disable geolocation
      'payment=()', // Disable payment API
      'usb=()', // Disable USB API
      'magnetometer=()', // Disable magnetometer
      'accelerometer=()', // Disable accelerometer
      'gyroscope=()', // Disable gyroscope
      'fullscreen=(self)', // Allow fullscreen only for same origin
      'autoplay=()', // Disable autoplay
    ].join(', ');

    res.setHeader('Permissions-Policy', permissionsPolicy);

    // Cross-Origin policies for enhanced security
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
    res.setHeader('Cross-Origin-Resource-Policy', 'same-origin');

    // Server information disclosure prevention
    res.removeHeader('X-Powered-By');
    res.removeHeader('Server');

    // Custom security headers for audit logging
    if (process.env.SECURITY_HEADERS_AUDIT === 'true') {
      res.setHeader('X-Security-Headers-Applied', 'true');
      res.setHeader(
        'X-Security-Policy-Version',
        process.env.SECURITY_POLICY_VERSION || '1.0',
      );
    }

    next();
  }
}
