# DNS Configuration for ID-Knuten Email Authentication

## Overview

This document provides the specific DNS records needed for the `idknuten.no` domain to enable proper email authentication for the ID-Knuten auth service using Resend.

**Domain**: `idknuten.no`  
**Email Service**: Resend  
**From Email**: `noreply@idknuten.no`  

## Required DNS Records

### 1. SPF (Sender Policy Framework)

**Purpose**: Authorizes Resend to send emails on behalf of `idknuten.no`

```dns
Type: TXT
Name: @ (or idknuten.no)
Value: v=spf1 include:_spf.resend.com ~all
TTL: 3600
```

**What this does:**
- `v=spf1` - SPF version 1
- `include:_spf.resend.com` - Include Resend's SPF record
- `~all` - Soft fail for emails not matching (recommended for initial setup)

### 2. DKIM (DomainKeys Identified Mail)

**Purpose**: Cryptographically signs emails to prevent tampering

```dns
Type: CNAME
Name: resend._domainkey
Value: resend._domainkey.resend.com
TTL: 3600
```

**Note**: This record will be provided by Resend after domain verification in their dashboard.

### 3. DMARC (Domain-based Message Authentication)

**Purpose**: Comprehensive email authentication policy

```dns
Type: TXT
Name: _dmarc
Value: v=DMARC1; p=quarantine; rua=mailto:dmarc-reports@idknuten.no; ruf=mailto:dmarc-failures@idknuten.no; fo=1; adkim=r; aspf=r; pct=100
TTL: 3600
```

**DMARC Policy Explanation:**
- `p=quarantine` - Quarantine suspicious emails (safer than reject for initial setup)
- `rua=` - Send aggregate reports to this email
- `ruf=` - Send forensic failure reports to this email
- `fo=1` - Generate reports for all authentication failures
- `adkim=r` - Relaxed DKIM alignment
- `aspf=r` - Relaxed SPF alignment
- `pct=100` - Apply policy to 100% of emails

### 4. Resend Domain Verification

**Purpose**: Verify domain ownership with Resend

```dns
Type: TXT
Name: @ (or idknuten.no)
Value: resend-domain-verify=[VERIFICATION_CODE_FROM_RESEND]
TTL: 3600
```

**Note**: The verification code will be provided by Resend when you add the domain to your account.

## Step-by-Step Setup Process

### Step 1: Add Domain to Resend Dashboard

1. Log into your Resend dashboard
2. Navigate to "Domains" section
3. Click "Add Domain"
4. Enter `idknuten.no`
5. Copy the verification TXT record provided

### Step 2: Configure DNS Records

Add all the above DNS records to your domain registrar's DNS management panel:

1. **SPF Record** - Add immediately
2. **Domain Verification** - Add the verification TXT record from Resend
3. **DKIM Record** - Add after domain is verified in Resend
4. **DMARC Record** - Add after SPF and DKIM are working

### Step 3: Verify Domain in Resend

1. After adding DNS records, click "Verify" in Resend dashboard
2. Wait for DNS propagation (can take up to 48 hours)
3. Resend will show "Verified" status when successful

### Step 4: Test Email Authentication

Use these tools to verify your setup:

1. **Mail Tester**: https://www.mail-tester.com/
2. **DMARC Analyzer**: https://www.dmarcanalyzer.com/
3. **MX Toolbox**: https://mxtoolbox.com/dmarc.aspx

## Environment Variables Validation

Your current `.env` configuration is correct for the DNS setup:

```bash
RESEND_FROM_EMAIL="noreply@idknuten.no"  ✅
RESEND_FROM_NAME="Id Knuten"             ✅
RESEND_SUPPORT_EMAIL="support@id-knuten.no"  ✅
```

**Note**: The support email uses `id-knuten.no` (with hyphen) while the from email uses `idknuten.no` (without hyphen). Ensure both domains are configured if you're using both.

## DMARC Monitoring Setup

Create these email addresses to receive DMARC reports:

1. **dmarc-reports@idknuten.no** - For aggregate reports
2. **dmarc-failures@idknuten.no** - For forensic reports

These can be:
- Forwarded to your main admin email
- Connected to a DMARC analysis service
- Monitored via email rules/filters

## DNS Propagation Check

After adding records, verify propagation:

```bash
# Check SPF record
dig TXT idknuten.no

# Check DKIM record
dig CNAME resend._domainkey.idknuten.no

# Check DMARC record
dig TXT _dmarc.idknuten.no
```

## Troubleshooting

### Common Issues

1. **Domain Not Verified**
   - Check DNS propagation (use dig commands above)
   - Ensure TXT record value matches exactly
   - Wait up to 48 hours for propagation

2. **DKIM Failure**
   - Verify CNAME record is correct
   - Check with Resend support if issues persist

3. **DMARC Policy Too Strict**
   - Start with `p=none` for monitoring only
   - Gradually move to `p=quarantine` then `p=reject`

### Production Recommendations

1. **Gradual DMARC Policy**:
   - Week 1: `p=none` (monitoring only)
   - Week 2-3: `p=quarantine; pct=25` (25% quarantine)
   - Week 4+: `p=quarantine; pct=100` (full quarantine)
   - Later: `p=reject` (when confident)

2. **Monitor Reports**:
   - Review DMARC aggregate reports weekly
   - Set up alerts for authentication failures
   - Monitor email deliverability metrics

## Security Considerations

1. **SPF Include Limit**: Maximum 10 DNS lookups in SPF record
2. **DKIM Key Rotation**: Resend handles this automatically
3. **DMARC Reporting**: Set up secure email for receiving reports
4. **Regular Monitoring**: Review authentication metrics monthly

## Production Checklist

- [ ] SPF record added and verified
- [ ] Domain verified in Resend dashboard
- [ ] DKIM record added and working
- [ ] DMARC record added with monitoring policy
- [ ] DMARC reporting emails configured
- [ ] Email authentication testing completed
- [ ] Monitoring and alerting set up

---

**Status**: Ready for DNS configuration  
**Domain**: idknuten.no  
**Email Service**: Resend  
**Next Step**: Add DNS records in domain registrar's control panel