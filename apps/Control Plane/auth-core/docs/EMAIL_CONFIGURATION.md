# Email Configuration for ID-Knuten Auth Service

## Overview

This document outlines the email configuration and authentication setup required for the ID-Knuten authentication service to achieve production-grade email deliverability and security compliance.

## Norwegian Email Templates ✅

The ID-Knuten auth service includes comprehensive Norwegian email templates with:

- **Email Verification**: Professional verification emails with security notices
- **Password Reset**: Secure password reset with warnings and expiration info
- **OTP Delivery**: One-time password emails with clear formatting
- **Account Management**: Change notifications and deletion confirmations

All templates include:
- Modern, responsive HTML design with inline CSS
- Plain text fallbacks for accessibility
- Professional Norwegian language with proper formal tone
- Security indicators and expiration warnings
- Consistent branding with ID-Knuten corporate identity

## Email Service Provider: Resend

**Production Configuration Required:**

### Environment Variables
```bash
# Required for Resend integration
RESEND_API_KEY=re_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
RESEND_FROM_NAME="ID-Knuten"
RESEND_FROM_EMAIL="auth@id-knuten.no"

# Optional security headers
SECURITY_HEADERS_AUDIT=true
SECURITY_POLICY_VERSION=1.0
```

## DNS Configuration for Email Authentication

### 1. SPF (Sender Policy Framework)

Add TXT record to `id-knuten.no` domain:

```dns
Type: TXT
Name: @ (or id-knuten.no)
Value: v=spf1 include:_spf.resend.com ~all
```

**Purpose**: Prevents email spoofing by specifying authorized sending servers.

### 2. DKIM (DomainKeys Identified Mail)

Resend provides DKIM records after domain verification:

```dns
Type: CNAME
Name: resend._domainkey
Value: resend._domainkey.resend.com
```

**Purpose**: Cryptographically signs emails to verify authenticity and integrity.

### 3. DMARC (Domain-based Message Authentication)

Add DMARC policy for comprehensive protection:

```dns
Type: TXT
Name: _dmarc
Value: v=DMARC1; p=reject; rua=mailto:dmarc-reports@id-knuten.no; ruf=mailto:dmarc-failures@id-knuten.no; fo=1
```

**Configuration Explanation:**
- `p=reject`: Reject emails that fail DMARC authentication
- `rua=`: Aggregate reports destination
- `ruf=`: Forensic failure reports destination
- `fo=1`: Generate reports for all authentication failures

### 4. Domain Verification

Add Resend domain verification record:

```dns
Type: TXT
Name: @ (or id-knuten.no)
Value: resend-domain-verify=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

## Security Features Implemented

### Email Authentication Headers

Each email includes security headers for better deliverability:

```typescript
const headers = {
  'X-Entity-Ref-ID': `verification-${Date.now()}`,
  'X-Priority': '1',
  'X-MSMail-Priority': 'High',
  Importance: 'high',
};
```

### Template Security Features

1. **Expiration Notices**: Clear communication of link/code expiry times
2. **Security Warnings**: Prominent warnings about suspicious requests
3. **Contact Information**: Clear support contact for security concerns
4. **Phishing Protection**: Consistent branding and URL patterns

## Production Checklist

### Pre-Launch Requirements

- [ ] **Domain Setup**: Configure SPF, DKIM, and DMARC records
- [ ] **Resend Account**: Verify domain ownership in Resend dashboard
- [ ] **Environment Variables**: Set production RESEND_API_KEY and email addresses
- [ ] **DMARC Monitoring**: Set up email monitoring for DMARC reports
- [ ] **Rate Limiting**: Configure Resend rate limits for production volume

### Post-Launch Monitoring

- [ ] **Deliverability Metrics**: Monitor email delivery rates
- [ ] **DMARC Reports**: Review weekly DMARC aggregate reports
- [ ] **Bounce Handling**: Implement bounce and complaint handling
- [ ] **Security Monitoring**: Monitor for email spoofing attempts

## Compliance Considerations

### GDPR Compliance

- Email templates include clear unsubscribe mechanisms where applicable
- Personal data handling aligned with GDPR requirements
- Clear data retention policies for email logs

### Norwegian Regulations

- Templates use formal Norwegian language appropriate for official communications
- Company information includes Norwegian address and contact details
- Compliance with Norwegian data protection authority (Datatilsynet) guidelines

## Testing and Validation

### Pre-Production Testing

1. **Template Rendering**: Test all email templates in multiple clients
2. **Authentication**: Verify DKIM signatures and SPF records
3. **Deliverability**: Test with major email providers (Gmail, Outlook, etc.)
4. **Security**: Validate DMARC policy effectiveness

### Production Monitoring

1. **Email Logs**: Monitor Resend webhook events
2. **Authentication Metrics**: Track SPF/DKIM/DMARC pass rates
3. **User Feedback**: Monitor support requests related to email delivery
4. **Security Alerts**: Set up alerts for authentication failures

## Support and Troubleshooting

### Common Issues

1. **Email Not Delivered**: Check SPF/DKIM/DMARC configuration
2. **Marked as Spam**: Review email content and authentication records
3. **Template Issues**: Validate HTML and ensure proper encoding

### Support Contacts

- **Resend Support**: support@resend.com
- **DMARC Analysis**: dmarc-analyzer.com for policy testing
- **Email Authentication**: mail-tester.com for comprehensive testing

## Environment-Specific Configuration

### Development
```bash
# Use Resend test domain
RESEND_FROM_EMAIL="onboarding@resend.dev"
RESEND_FROM_NAME="ID-Knuten Dev"
```

### Production
```bash
# Use verified domain
RESEND_FROM_EMAIL="auth@id-knuten.no"
RESEND_FROM_NAME="ID-Knuten"
```

## Security Recommendations

1. **API Key Security**: Store Resend API key in secure environment variables
2. **Rate Limiting**: Implement application-level rate limiting for email sending
3. **Monitoring**: Set up alerts for unusual email sending patterns
4. **Backup Provider**: Consider secondary email provider for redundancy

---

**Status**: ✅ Norwegian templates implemented and integrated with Resend service
**Next Steps**: DNS configuration and domain verification in production environment