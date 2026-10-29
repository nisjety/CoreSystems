#!/bin/bash

# User Service gRPC Test Script
# Tests all implemented gRPC endpoints

set -e

GRPC_PORT=50052
SERVER="localhost:$GRPC_PORT"
INTERNAL_API_KEY="${INTERNAL_API_KEY:-${INTERNAL_SERVICE_SECRET:-}}"

if [ -z "$INTERNAL_API_KEY" ]; then
    echo "❌ INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for user-core gRPC calls"
    exit 1
fi

echo "🧪 Testing User Service gRPC API on $SERVER"
echo "================================================"

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

test_endpoint() {
    local name=$1
    local method=$2
    local data=$3
    
    echo -e "\n📡 Testing: $name"
    echo "Method: $method"
    
    if grpcurl -plaintext -H "x-internal-api-key: $INTERNAL_API_KEY" -d "$data" "$SERVER" "$method"; then
        echo -e "${GREEN}✅ Success${NC}"
        return 0
    else
        echo -e "${RED}❌ Failed${NC}"
        return 1
    fi
}

echo -e "\n1️⃣  Health Check"
test_endpoint "Health Check" \
    "user.v1.UserService/HealthCheck" \
    '{"service": "user-service"}'

echo -e "\n2️⃣  Create User"
test_endpoint "Create User" \
    "user.v1.UserService/CreateUser" \
    '{"email": "test@example.com", "name": "Test User", "password": "password123", "avatar": "https://avatar.com/test.jpg"}'

echo -e "\n3️⃣  Get User by Email"
test_endpoint "Get User by Email" \
    "user.v1.UserService/GetUserByEmail" \
    '{"email": "test@example.com"}'

# Extract user ID from previous response (simplified - in real test would parse JSON)
USER_ID="test-user-id"

echo -e "\n4️⃣  Get User by ID"
test_endpoint "Get User by ID" \
    "user.v1.UserService/GetUser" \
    "{\"id\": \"$USER_ID\"}"

echo -e "\n5️⃣  Update User"
test_endpoint "Update User" \
    "user.v1.UserService/UpdateUser" \
    "{\"id\": \"$USER_ID\", \"name\": \"Updated Name\"}"

echo -e "\n6️⃣  List Users"
test_endpoint "List Users" \
    "user.v1.UserService/ListUsers" \
    '{"pagination": {"page": 1, "limit": 10}}'

echo -e "\n7️⃣  Block User"
test_endpoint "Block User" \
    "user.v1.UserService/BlockUser" \
    "{\"user_id\": \"$USER_ID\", \"reason\": \"Test block\"}"

echo -e "\n8️⃣  Unblock User"
test_endpoint "Unblock User" \
    "user.v1.UserService/UnblockUser" \
    "{\"user_id\": \"$USER_ID\"}"

echo -e "\n9️⃣  Suspend User"
test_endpoint "Suspend User" \
    "user.v1.UserService/SuspendUser" \
    "{\"user_id\": \"$USER_ID\", \"reason\": \"Test suspension\"}"

echo -e "\n🔟 Unsuspend User"
test_endpoint "Unsuspend User" \
    "user.v1.UserService/UnsuspendUser" \
    "{\"user_id\": \"$USER_ID\"}"

echo -e "\n1️⃣1️⃣  Deactivate User"
test_endpoint "Deactivate User" \
    "user.v1.UserService/DeactivateUser" \
    "{\"user_id\": \"$USER_ID\"}"

echo -e "\n1️⃣2️⃣  Activate User"
test_endpoint "Activate User" \
    "user.v1.UserService/ActivateUser" \
    "{\"user_id\": \"$USER_ID\"}"

echo -e "\n1️⃣3️⃣  Get User Profile"
test_endpoint "Get User Profile" \
    "user.v1.UserService/GetUserProfile" \
    "{\"user_id\": \"$USER_ID\"}"

echo -e "\n1️⃣4️⃣  Update User Profile"
test_endpoint "Update User Profile" \
    "user.v1.UserService/UpdateUserProfile" \
    "{\"user_id\": \"$USER_ID\", \"bio\": \"Test bio\", \"location\": \"Oslo, Norway\"}"

echo -e "\n1️⃣5️⃣  Delete User"
test_endpoint "Delete User" \
    "user.v1.UserService/DeleteUser" \
    "{\"id\": \"$USER_ID\"}"

echo -e "\n================================================"
echo -e "${GREEN}✅ All basic tests completed${NC}"
echo ""
echo "📋 Note: Session, Activity, Role, and Device management"
echo "   are marked as 'Unimplemented' and will be added later."
echo ""
echo "To list all available methods:"
echo "  grpcurl -plaintext -H \"x-internal-api-key: \$INTERNAL_API_KEY\" $SERVER list user.v1.UserService"
echo ""
echo "To describe a method:"
echo "  grpcurl -plaintext -H \"x-internal-api-key: \$INTERNAL_API_KEY\" $SERVER describe user.v1.UserService.CreateUser"
