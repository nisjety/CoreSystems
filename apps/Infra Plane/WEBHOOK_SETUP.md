# Auto-Deployment Setup Guide

## ✅ Webhook Service Status

The webhook service is now running on your VPS and ready to receive GitHub push events.

**Service Details:**
- Status: ✅ **Active and running**
- Port: 9000
- VPS IP: 31.97.38.31
- Webhook URL: `http://31.97.38.31:9000`
- Webhook Secret: `0d7c991d7ea6dad2935ed527172f0779251aef148dc6050bb3738466536`

## 🔧 Configure GitHub Webhook

To enable automatic deployment when you push to the `main` branch:

### Step 1: Go to GitHub Repository Settings
1. Navigate to: https://github.com/I-Dacosta/Aquatiq/settings/hooks
2. Click **"Add webhook"** button

### Step 2: Configure Webhook
Fill in the following fields:

**Payload URL:**
```
http://31.97.38.31:9000
```

**Content type:**
- Select: `application/json`

**Secret:**
```
0d7c991d7ea6dad2935ed527172f0779251aef148dc6050bb3738466536
```

**Which events would you like to trigger this webhook?**
- Select: **"Let me select individual events"**
- ✓ Check: **Push events**
- Uncheck: Pull requests, Issues, etc. (optional)

**Active:**
- ✓ Check: **Active**

### Step 3: Save Webhook
Click **"Add webhook"** button to create the webhook.

## 📋 How It Works

When you push changes to the `main` branch (production deployments):

1. **GitHub sends webhook event** → VPS receives it on port 9000
2. **Webhook verifies signature** → Ensures it's really from GitHub
3. **Git pulls latest code** → Updates `/opt/coresystem` directory
4. **Docker images rebuild** → Only changed services are rebuilt
5. **Services restart** → Updated containers start automatically
6. **Health checks run** → Verifies deployment success

### Example Workflow

```bash
# On your local machine
cd /Volumes/Lagring/Aquatiq/coresystem-root-container

# For development work (use dev branch)
git checkout dev
vim docker-compose.yml
git add docker-compose.yml
git commit -m "Update service configuration"
git push origin dev

# When ready for production, merge dev to main
git checkout main
git merge dev
git push origin main

# 🎉 VPS automatically deploys the changes!
```

## 📊 Monitoring Deployment

### View Webhook Logs
```bash
ssh root@31.97.38.31 "sudo journalctl -u coresystem-webhook -f"
```

### Check Last Deployment
```bash
ssh root@31.97.38.31 "tail -50 /var/log/coresystem-webhook.log"
```

### Verify Service Running
```bash
ssh root@31.97.38.31 "sudo systemctl status coresystem-webhook"
```

## 🔒 Security Features

- ✅ **GitHub HMAC Signature Verification** - Ensures webhooks are authentic
- ✅ **Firewall Restricted** - Only GitHub IP ranges can access webhook port
- ✅ **Atomic Git Operations** - Stashes local changes before pulling
- ✅ **Systemd Service** - Auto-restarts if service crashes
- ✅ **Comprehensive Logging** - All deployments logged to `/var/log/coresystem-webhook.log`

## 🚨 Troubleshooting

### Webhook Not Triggering?
1. Check webhook is active in GitHub settings
2. Verify you're pushing to `main` branch (not `dev` or other branches)
3. Check VPS can be reached: `curl http://31.97.38.31:9000`

### Deployment Fails?
1. Check logs: `sudo journalctl -u coresystem-webhook -f`
2. Verify git repository is clean: `cd /opt/coresystem && git status`
3. Check docker compose syntax: `docker compose -f /opt/coresystem/docker-compose.yml config`

### Service Won't Start?
```bash
# Restart service
sudo systemctl restart coresystem-webhook

# View errors
sudo systemctl status coresystem-webhook
```

## 📝 Manual Deployment

If you need to deploy without pushing to GitHub:

```bash
ssh root@31.97.38.31 "/opt/coresystem/webhook-deploy.sh deploy"
```

## 🔄 Webhook Secret Rotation

If you want to rotate the webhook secret for security:

```bash
ssh root@31.97.38.31 "openssl rand -hex 32 > /opt/coresystem/.webhook-secret"
```

Then update the secret in GitHub webhook settings.

---

**Status:** ✅ Ready to deploy  
**Last Updated:** February 2, 2026  
**Deployment Branch:** `main` (production)  
**Development Branch:** `dev`
