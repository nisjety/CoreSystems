# Teams Gateway Service

The Teams Gateway Service is a NestJS microservice that handles Microsoft Teams integration for the Aquatiq Chat system. It processes incoming messages from Teams and coordinates with the AI Orchestrator service.

## Features

- 🤖 Microsoft Bot Framework integration
- 💬 Teams message processing  
- 🔒 GDPR-compliant data handling
- 📊 PostgreSQL database integration
- 🚀 NestJS microservices architecture

## Quick Start

### Prerequisites

- Node.js 18+ 
- PostgreSQL 16
- Docker (for database infrastructure)

### Installation

1. **Install dependencies:**
   ```bash
   npm install
   ```

2. **Start the database infrastructure:**
   ```bash
   # From the project root
   docker-compose -f docker-compose.dev.yml up -d
   ```

3. **Configure environment:**
   ```bash
   cp .env.example .env
   # Edit .env with your configuration
   ```

4. **Build the application:**
   ```bash
   npm run build
   ```

5. **Start the service:**
   ```bash
   npm run start:dev
   ```

The service will be available at `http://localhost:3031`

## Bot Testing

### Local Testing Script

Use the included test script to simulate Teams messages locally:

```bash
# Start the bot tester (in a separate terminal)
npm run test:bot

# Send a test message
curl -X POST http://localhost:3999/test/send \
  -H "Content-Type: application/json" \
  -d '{"message":"Hello Aquatiq Bot!"}'
```

### Testing Flow

1. Start the Teams Gateway service: `npm run start:dev`
2. Start the bot tester: `npm run test:bot` 
3. Send test messages via the tester API
4. Check the Teams Gateway logs for message processing

## API Endpoints

### Bot Framework Endpoint

- **POST** `/api/messages` - Microsoft Bot Framework activity endpoint

## Database Schema

The service uses the following main entities:

- **Users** - Teams user information with GDPR compliance
- **Conversations** - Teams conversation contexts  
- **Messages** - Message history with encryption support

## Environment Variables

```bash
# Server Configuration
PORT=3031
NODE_ENV=development

# Microsoft Bot Framework
MICROSOFT_APP_ID=          # Bot App ID (empty for local dev)
MICROSOFT_APP_PASSWORD=    # Bot App Password (empty for local dev)

# Database Configuration
DB_HOST=localhost
DB_PORT=5432
DB_USERNAME=aquatiq_user
DB_PASSWORD=secure_password_123
DB_DATABASE=aquatiq_chat

# Services
ORCHESTRATOR_SERVICE_URL=http://localhost:3001
```

## Development Commands

```bash
# Development
npm run start:dev          # Start with hot reload
npm run start:debug        # Start with debugging

# Building
npm run build              # Build the application
npm run start:prod         # Start production build

# Testing
npm run test               # Run unit tests
npm run test:e2e           # Run end-to-end tests
npm run test:bot           # Start bot testing script

# Code Quality
npm run lint               # Lint the code
npm run format             # Format the code
```

## Architecture

```
Teams Gateway Service
├── src/
│   ├── adapters/          # Bot Framework adapters
│   ├── bot/               # Bot controllers and services
│   ├── config/            # Configuration classes
│   ├── entities/          # TypeORM entities
│   └── services/          # Business logic services
├── test-bot.js            # Local testing script
└── .env                   # Environment configuration
```

## Security Features

- 🔐 Encrypted user data vaults
- 📝 Audit logging for all interactions
- 🛡️ GDPR-compliant data handling
- 🔑 JWT-based authentication ready
- 🌐 CORS configuration for Teams

## Next Steps

1. **AI Integration**: Connect with the AI Orchestrator service
2. **Authentication**: Add Microsoft Teams authentication
3. **Rich Responses**: Implement Adaptive Cards
4. **File Handling**: Add support for file uploads
5. **Azure Deployment**: Deploy to Azure Container Apps

## Troubleshooting

### Common Issues

1. **Database Connection Failed**
   - Ensure PostgreSQL is running via Docker Compose
   - Check database credentials in `.env`

2. **Build Errors**
   - Run `npm install` to ensure all dependencies are installed
   - Check TypeScript compilation with `npm run build`

3. **Bot Not Responding**
   - Verify the service is running on port 3031
   - Check the bot tester is sending to the correct endpoint
   - Review logs for any error messages

### Logs

The service logs all Bot Framework activities and database operations. Check the console output for detailed information about message processing.

---

**Part of the Aquatiq Chat System** 🌊
