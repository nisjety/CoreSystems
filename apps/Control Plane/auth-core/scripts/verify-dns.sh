#!/bin/bash

# DNS Configuration Verification Script for ID-Knuten
# This script checks if the required DNS records are properly configured

echo "🔍 DNS Configuration Verification for idknuten.no"
echo "=================================================="
echo ""

DOMAIN="idknuten.no"
SUBDOMAIN_DKIM="resend._domainkey.${DOMAIN}"
SUBDOMAIN_DMARC="_dmarc.${DOMAIN}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}1. Checking SPF Record...${NC}"
SPF_RECORD=$(dig +short TXT $DOMAIN | grep "v=spf1")
if [ -n "$SPF_RECORD" ]; then
    if echo "$SPF_RECORD" | grep -q "include:_spf.resend.com"; then
        echo -e "   ${GREEN}✅ SPF Record Found: $SPF_RECORD${NC}"
    else
        echo -e "   ${YELLOW}⚠️  SPF Record Found but missing Resend: $SPF_RECORD${NC}"
    fi
else
    echo -e "   ${RED}❌ No SPF Record Found${NC}"
fi
echo ""

echo -e "${BLUE}2. Checking DKIM Record...${NC}"
DKIM_RECORD=$(dig +short CNAME $SUBDOMAIN_DKIM)
if [ -n "$DKIM_RECORD" ]; then
    if echo "$DKIM_RECORD" | grep -q "resend._domainkey.resend.com"; then
        echo -e "   ${GREEN}✅ DKIM Record Found: $DKIM_RECORD${NC}"
    else
        echo -e "   ${YELLOW}⚠️  DKIM Record Found but incorrect: $DKIM_RECORD${NC}"
    fi
else
    echo -e "   ${RED}❌ No DKIM Record Found${NC}"
fi
echo ""

echo -e "${BLUE}3. Checking DMARC Record...${NC}"
DMARC_RECORD=$(dig +short TXT $SUBDOMAIN_DMARC)
if [ -n "$DMARC_RECORD" ]; then
    if echo "$DMARC_RECORD" | grep -q "v=DMARC1"; then
        echo -e "   ${GREEN}✅ DMARC Record Found: $DMARC_RECORD${NC}"
    else
        echo -e "   ${YELLOW}⚠️  DMARC Record Found but invalid: $DMARC_RECORD${NC}"
    fi
else
    echo -e "   ${RED}❌ No DMARC Record Found${NC}"
fi
echo ""

echo -e "${BLUE}4. Checking Domain Verification Record...${NC}"
VERIFICATION_RECORD=$(dig +short TXT $DOMAIN | grep "resend-domain-verify")
if [ -n "$VERIFICATION_RECORD" ]; then
    echo -e "   ${GREEN}✅ Domain Verification Record Found: $VERIFICATION_RECORD${NC}"
else
    echo -e "   ${RED}❌ No Domain Verification Record Found${NC}"
fi
echo ""

echo -e "${BLUE}5. DNS Propagation Status...${NC}"
echo "   Checking global DNS propagation..."
echo "   You can manually check at: https://whatsmydns.net/"
echo ""

echo -e "${YELLOW}📋 Required DNS Records Summary:${NC}"
echo "=================================="
echo ""
echo "SPF Record:"
echo "Type: TXT"
echo "Name: @ (or $DOMAIN)"
echo "Value: v=spf1 include:_spf.resend.com ~all"
echo ""
echo "DKIM Record:"
echo "Type: CNAME"
echo "Name: resend._domainkey"
echo "Value: resend._domainkey.resend.com"
echo ""
echo "DMARC Record:"
echo "Type: TXT"
echo "Name: _dmarc"
echo "Value: v=DMARC1; p=quarantine; rua=mailto:dmarc-reports@$DOMAIN; ruf=mailto:dmarc-failures@$DOMAIN; fo=1"
echo ""
echo "Domain Verification (from Resend dashboard):"
echo "Type: TXT"
echo "Name: @ (or $DOMAIN)"
echo "Value: resend-domain-verify=[CODE_FROM_RESEND]"
echo ""

echo -e "${BLUE}📧 Email Testing:${NC}"
echo "================"
echo "After DNS records are configured, test with:"
echo "• https://www.mail-tester.com/"
echo "• https://www.dmarcanalyzer.com/"
echo "• https://mxtoolbox.com/dmarc.aspx"
echo ""

echo -e "${GREEN}🎯 Next Steps:${NC}"
echo "=============="
echo "1. Add the DNS records shown above to your domain registrar"
echo "2. Verify domain in Resend dashboard"
echo "3. Wait for DNS propagation (up to 48 hours)"
echo "4. Run this script again to verify configuration"
echo "5. Test email sending from the application"
echo ""

echo "Script completed at $(date)"