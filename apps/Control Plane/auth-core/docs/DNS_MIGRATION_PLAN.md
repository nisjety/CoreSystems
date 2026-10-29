# DNS Migration Plan for ID-Knuten Email Authentication

## Current DNS Status (Verified September 14, 2025)

**Domain**: `idknuten.no`

### ✅ Existing Records (Good)
- **DMARC**: `v=DMARC1; p=none;` (Basic policy in place)

### ⚠️ Records Needing Updates
- **SPF**: Currently has `v=spf1 +a +mx +ip4:185.126.36.11 include:_spf.spamprotection.no ~all`
  - **Action Required**: Add Resend to existing SPF record

### ❌ Missing Records
- **DKIM**: No `resend._domainkey` CNAME record
- **Domain Verification**: No Resend verification TXT record

## Migration Steps

### Step 1: Update SPF Record (CRITICAL)

**Current SPF Record:**
```
v=spf1 +a +mx +ip4:185.126.36.11 include:_spf.spamprotection.no ~all
```

**Updated SPF Record (Required):**
```
v=spf1 +a +mx +ip4:185.126.36.11 include:_spf.spamprotection.no include:_spf.resend.com ~all
```

**DNS Configuration:**
```
Type: TXT
Name: @ (or idknuten.no)
Value: v=spf1 +a +mx +ip4:185.126.36.11 include:_spf.spamprotection.no include:_spf.resend.com ~all
TTL: 3600
```

⚠️ **Important**: This preserves your existing mail setup while adding Resend authorization.

### Step 2: Add Resend Domain to Dashboard

1. **Login to Resend Dashboard**: https://resend.com/domains
2. **Add Domain**: Click "Add Domain" and enter `idknuten.no`
3. **Copy Verification Code**: Resend will provide a verification TXT record
4. **Example**: `resend-domain-verify=abc123def456...`

### Step 3: Add Domain Verification Record

```
Type: TXT
Name: @ (or idknuten.no)
Value: resend-domain-verify=[CODE_FROM_RESEND_DASHBOARD]
TTL: 3600
```

### Step 4: Add DKIM Record

**After domain is verified in Resend:**
```
Type: CNAME
Name: resend._domainkey
Value: resend._domainkey.resend.com
TTL: 3600
```

### Step 5: Enhance DMARC Record (Optional but Recommended)

**Current DMARC:**
```
v=DMARC1; p=none;
```

**Enhanced DMARC (Recommended):**
```
v=DMARC1; p=quarantine; rua=mailto:dmarc-reports@idknuten.no; ruf=mailto:dmarc-failures@idknuten.no; fo=1; adkim=r; aspf=r; pct=25
```

**DNS Configuration:**
```
Type: TXT
Name: _dmarc
Value: v=DMARC1; p=quarantine; rua=mailto:dmarc-reports@idknuten.no; ruf=mailto:dmarc-failures@idknuten.no; fo=1; adkim=r; aspf=r; pct=25
TTL: 3600
```

## Risk Assessment

### Low Risk
- **SPF Update**: Adding Resend include is safe and preserves existing mail flow
- **DKIM Addition**: New record, won't affect existing email
- **Domain Verification**: Temporary record for Resend verification

### Medium Risk
- **DMARC Enhancement**: Moving from `p=none` to `p=quarantine` affects email delivery
  - **Recommendation**: Start with `pct=25` (25% policy application)
  - **Monitor**: Review DMARC reports before increasing percentage

## Timeline and Rollback Plan

### Phase 1: Safe Additions (Day 1)
1. Update SPF record to include Resend
2. Add domain verification TXT record
3. Verify domain in Resend dashboard
4. Add DKIM CNAME record

### Phase 2: Verification (Day 2-3)
1. Wait for DNS propagation (24-48 hours)
2. Run verification script: `./scripts/verify-dns.sh`
3. Test email sending from application
4. Verify DKIM signatures are working

### Phase 3: DMARC Enhancement (Week 2)
1. Update DMARC policy to `p=quarantine; pct=25`
2. Monitor email deliverability
3. Review DMARC reports
4. Gradually increase `pct` value

### Rollback Plan
If issues occur:
1. **SPF Rollback**: Remove `include:_spf.resend.com` from SPF record
2. **DMARC Rollback**: Revert to `p=none`
3. **Remove Records**: Delete DKIM and verification records if needed

## Monitoring and Validation

### Immediate Testing (After DNS Propagation)
```bash
# Run verification script
./scripts/verify-dns.sh

# Manual DNS checks
dig TXT idknuten.no | grep spf
dig CNAME resend._domainkey.idknuten.no
dig TXT _dmarc.idknuten.no
```

### Email Testing Tools
1. **Send test email** from application
2. **Mail Tester**: https://www.mail-tester.com/
3. **DMARC Analyzer**: https://www.dmarcanalyzer.com/
4. **MX Toolbox**: https://mxtoolbox.com/dmarc.aspx

### Production Monitoring
1. **Resend Dashboard**: Monitor delivery rates and bounces
2. **DMARC Reports**: Check `dmarc-reports@idknuten.no` weekly
3. **Application Logs**: Monitor email sending success rates
4. **User Feedback**: Watch for email delivery complaints

## Contact Information

### DNS Management
- **Domain Registrar**: [Contact your registrar for DNS changes]
- **Current Email Provider**: spamprotection.no (based on SPF record)

### Email Service
- **Resend Support**: support@resend.com
- **Resend Documentation**: https://resend.com/docs

### Emergency Contacts
If email delivery stops working:
1. **Immediate**: Revert SPF record to original value
2. **Check**: Application logs for email sending errors
3. **Contact**: Resend support with domain and configuration details

## Success Criteria

### DNS Configuration Complete When:
- [ ] SPF record includes both existing and Resend includes
- [ ] Domain verified in Resend dashboard (green checkmark)
- [ ] DKIM record resolves correctly
- [ ] Verification script shows all green checkmarks
- [ ] Test emails from application are delivered successfully
- [ ] Mail-tester.com gives 10/10 score

### Production Ready When:
- [ ] All DNS records propagated globally
- [ ] Email templates working with Norwegian content
- [ ] DMARC reports being received and reviewed
- [ ] No delivery issues reported by users
- [ ] Backup email monitoring in place

---

**Created**: September 14, 2025  
**Domain**: idknuten.no  
**Status**: Ready for DNS migration  
**Risk Level**: Low (preserving existing email setup)