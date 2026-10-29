# ⚠️ You Need SuperOffice OAuth Credentials

Before running the OAuth flow, you need to:

## Step 1: Get OAuth Credentials from SuperOffice

1. Go to: https://devnet.superoffice.com/
2. Log in with your SuperOffice account
3. Find your application (or create one)
4. Note down:
   - **App ID** (Client ID)
   - **App Secret** (Client Secret)

## Step 2: Configure the Redirect URL

In your SuperOffice app settings on devnet.superoffice.com:

1. Find the "Redirect URLs" section
2. Add: `http://localhost:5001/callback`
3. Save the changes

This is **required** for the OAuth flow to work!

## Step 3: Update .env File

Update these values in `.env`:

```
SOCRM_CLIENT_ID=your_app_id_from_superoffice
SOCRM_CLIENT_SECRET=your_app_secret_from_superoffice
SOCRM_REDIRECT_URI=http://localhost:5001/callback
```

## Step 4: Run OAuth Flow

```bash
source venv/bin/activate
python oauth_simple.py
```

Then log in with your SuperOffice account.

---

## Need Help?

- SuperOffice Developer Portal: https://devnet.superoffice.com/
- Make sure the app is enabled
- Check that you added the redirect URL correctly
- The OAuth login should redirect back automatically

Once you have the credentials configured, run the script again!
