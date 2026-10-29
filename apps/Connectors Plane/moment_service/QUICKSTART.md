# Moment Service - Quick Start Guide

Get up and running with the Moment integration service in 5 minutes!

## 🚀 Quick Setup

### 1. Configure Your Credentials

You need two things from Moment:

1. **API Key**: Generated in Moment under Company > API Keys
2. **Company Code**: Found in your Moment URL

Edit the `.env` file:

```bash
cd /Volumes/Lagring/Aquatiq/Aquatiq\ integrasjonen\ /apps/moment_service
```

Update these values in `.env`:

```env
MOMENT_API_KEY=your_api_key_here
MOMENT_COMPANY_CODE=your_company_code
```

**Finding your company code:**
```
https://app.moment.team/team/cloudware/...
                              ^^^^^^^^^
                              This is your company code
```

### 2. Choose Your Runtime

#### Option A: Docker (Recommended)

```bash
# Start all services (Moment, Redis, PostgreSQL)
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f moment_service
```

#### Option B: Local Python

```bash
# Create virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt

# Run the service
python app.py
```

#### Option C: Use Quick Start Script

```bash
./quick-start.sh
```

## ✅ Verify Installation

### Test the API Connection

```bash
# Run the test script
python test_moment_api.py
```

### Check Health

```bash
curl http://localhost:5006/health
```

Expected response:
```json
{
  "status": "healthy",
  "service": "moment_service",
  "version": "1.0.0",
  "timestamp": "2024-12-22T10:30:00.000Z"
}
```

## 📋 Common API Examples

### Get Projects

```bash
curl http://localhost:5006/api/projects?includes=base
```

### Get Time Records

```bash
curl http://localhost:5006/api/time-records?includes=base
```

### Create Time Record

```bash
curl -X POST http://localhost:5006/api/time-records \
  -H "Content-Type: application/json" \
  -d '{
    "projectId": 123,
    "date": "2024-12-22",
    "hours": 8,
    "description": "Development work"
  }'
```

### Get Registered Hours Report

```bash
curl "http://localhost:5006/api/projects/registered-hours-report?fromDate=2024-12-01&toDate=2024-12-31&billableRateOption=registered"
```

### Get Customers

```bash
curl http://localhost:5006/api/customers?includes=base
```

### Get User Accounts

```bash
curl http://localhost:5006/api/user-accounts?includes=base
```

## 🎯 Service URLs

Once running, you can access:

| Service | URL |
|---------|-----|
| API Base | http://localhost:5006/api |
| Health Check | http://localhost:5006/health |
| Status | http://localhost:5006/api/status |

## 🔧 Configuration Quick Reference

| Environment Variable | Required | Default | Description |
|---------------------|----------|---------|-------------|
| `MOMENT_API_KEY` | ✅ Yes | - | Your Moment API key |
| `MOMENT_COMPANY_CODE` | ✅ Yes | - | Company code from Moment URL |
| `MOMENT_ENVIRONMENT` | No | `prod` | `prod`, `sandbox`, or `all` |
| `MOMENT_PORT` | No | `5006` | Service port |

## 🐛 Troubleshooting

### "API key is required" Error

Make sure `MOMENT_API_KEY` is set in `.env`:

```bash
echo $MOMENT_API_KEY  # Should output your key
```

### "Company code is required" Error

Set `MOMENT_COMPANY_CODE` in `.env`:

```bash
# Find it in your Moment URL
# https://app.moment.team/team/{company_code}/...
```

### Authentication Error (401)

- Verify API key is correct
- Check that API Access is enabled in Moment
- Ensure API key hasn't expired (max 2 years)

### Connection Error

- Check internet connection
- Verify `MOMENT_BASE_URL` is correct
- Test with: `curl https://app.moment.team/api/1.0/`

### Docker Issues

```bash
# Restart services
docker-compose restart

# Rebuild image
docker-compose build --no-cache
docker-compose up -d

# Check logs
docker-compose logs moment_service
```

## 📚 Next Steps

1. **Read the full documentation**: See [README.md](README.md)
2. **Explore API endpoints**: Check the API Documentation section
3. **Integrate with your app**: Use the REST API from your application
4. **Set up sync**: Configure BigQuery or PostgreSQL sync (optional)

## 💡 Tips

- Use `includes` parameter to optimize data fetching
- Implement batch operations for bulk updates
- Monitor rate limits (30 req/sec per token)
- Cache frequently accessed data
- Use pagination for large datasets

## 🆘 Getting Help

- Check [README.md](README.md) for detailed documentation
- Run `python test_moment_api.py` to diagnose issues
- View logs: `docker-compose logs -f moment_service`
- Check Moment API docs: https://app.moment.team/api/1.0/docs/

## 🎉 You're Ready!

The Moment service is now ready to use. Start making API calls and integrate it with your applications!

```bash
# Example: Get all projects
curl http://localhost:5006/api/projects

# Example: Get time records for today
curl "http://localhost:5006/api/time-records?date=$(date +%Y-%m-%d)"

# Example: Get registered hours for last month
curl "http://localhost:5006/api/projects/registered-hours-report?fromDate=2024-11-01&toDate=2024-11-30"
```

Happy coding! 🚀
