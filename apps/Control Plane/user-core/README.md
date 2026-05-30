# User Service (Go)

A high-performance Go-based user management service with gRPC support for the Aquatiq CMS ecosystem.

## Features

- **Complete User Management**: CRUD operations with comprehensive profile management
- **Session Management**: Track and manage user sessions across devices
- **Activity Logging**: Comprehensive user activity tracking and audit trails
- **Role-Based Access Control**: Flexible role and permission management
- **Device Management**: Track and manage user devices for security
- **gRPC API**: High-performance gRPC endpoints for inter-service communication
- **PostgreSQL**: Robust data persistence with pgx driver
- **Type-Safe**: Full type safety with Protocol Buffers

## Tech Stack

- **Language**: Go 1.24+
- **RPC**: gRPC with Protocol Buffers
- **Database**: PostgreSQL with pgx/v5
- **Validation**: go-playground/validator
- **Configuration**: Viper

## Project Structure

```
apps/user-service-go/
├── cmd/
│   ├── server/          # gRPC server entry point
│   └── dual/            # Dual-mode (HTTP + gRPC) entry point
├── internal/
│   ├── users/           # User domain logic
│   │   ├── service.go   # User service implementation
│   │   ├── repository.go # Database operations
│   │   └── validation.go # Business validation
│   ├── sessions/        # Session management
│   ├── activities/      # Activity logging
│   ├── roles/           # Role management
│   ├── devices/         # Device management
│   ├── grpc/            # gRPC server implementation
│   │   ├── server.go    # Server setup
│   │   └── handlers.go  # RPC method handlers
│   ├── database/        # Database layer
│   └── config/          # Configuration
├── proto/user/v1/       # Protocol Buffer definitions
│   └── user.proto
├── migrations/          # Database migrations
├── go.mod
├── go.sum
├── Makefile
└── README.md
```

## Quick Start

### Prerequisites

- Go 1.24+
- PostgreSQL 14+
- Protocol Buffers compiler (protoc)

### Installation

1. **Install dependencies**:
   ```bash
   go mod download
   ```

2. **Set up environment**:
   ```bash
   cp .env.example .env
   # Edit .env with your database configuration
   ```

3. **Generate Proto code**:
   ```bash
   make proto-generate
   ```

4. **Run migrations**:
   ```bash
   make db-migrate
   ```

5. **Start the service**:
   ```bash
   make run
   # Or for development with hot reload:
   make dev
   ```

The service will be available on port `50052` (gRPC)

## gRPC API

### User Management
- `CreateUser` - Create new user
- `GetUser` - Get user by ID
- `GetUserByEmail` - Get user by email
- `UpdateUser` - Update user information
- `DeleteUser` - Delete user
- `ListUsers` - List users with pagination
- `ActivateUser` / `DeactivateUser` - User status management
- `BlockUser` / `UnblockUser` - User blocking
- `SuspendUser` / `UnsuspendUser` - User suspension

### Profile Management
- `GetUserProfile` - Get user profile
- `UpdateUserProfile` - Update user profile

### Session Management
- `CreateSession` - Create user session
- `ListSessions` - List user sessions
- `InvalidateSession` - Invalidate specific session
- `InvalidateAllSessions` - Invalidate all user sessions

### Activity Logging
- `LogActivity` - Log user activity
- `ListActivities` - Get user activities

### Role Management
- `AssignRole` - Assign role to user
- `ListUserRoles` - List user roles
- `RemoveRole` - Remove user role

### Device Management
- `RegisterDevice` - Register user device
- `ListDevices` - List user devices
- `UpdateDevice` - Update device information
- `DeactivateDevice` - Deactivate device

## Environment Configuration

```env
# Database
DATABASE_TYPE=postgres
DATABASE_URI=postgresql://postgres:password@localhost:5432/user_service

# Server
GRPC_PORT=50052
HTTP_PORT=3001

# Logging
LOG_LEVEL=info
```

## Development Commands

```bash
# Generate proto code
make proto-generate

# Build
make build

# Run
make run

# Run with hot reload
make dev

# Run tests
make test

# Database migrations
make db-migrate
make db-rollback
```

## Architecture

Follows Clean Architecture principles:
- **Domain Layer**: Business logic in `internal/users/`, `internal/sessions/`, etc.
- **Infrastructure Layer**: Database, gRPC in `internal/database/`, `internal/grpc/`
- **Interface Layer**: Proto definitions in `proto/user/v1/`

## Integration

This service integrates with:
- **Auth Service** (Port 50051): Authentication via Better Auth
- **CMS Service** (Port 50053): Content management
- **API Gateway** (Port 3002): oRPC → gRPC translation

## License

MIT
