//! Minimal, hand-rolled gRPC client for Control Plane's `user-core`, used
//! only to look up the acting user's own email and (optional) phone number
//! so `book_shipment` can supply a real carrier notification contact for
//! the recipient when the model itself has no reason to know one.
//!
//! This does not pull in `user-core`'s full proto surface (it lives in a
//! different plane's Go module, not this workspace) — it hand-encodes the
//! exact handful of fields this client needs via `prost::Message`, which is
//! forward-compatible by construction: protobuf's wire format lets a reader
//! declare a strict subset of a message's fields and ignore everything it
//! doesn't declare. Field numbers below are copied verbatim from
//! `user-core/proto/user/v1/*.proto`'s `User`/`UserProfile`/`GetUser*`
//! messages; only their meaning (not the tag number) may drift if the two
//! ever fall out of sync.
//!
//! Auth: user-core's gRPC layer (`internal/grpc/service_auth.go`) verifies
//! three plain metadata values against a static, deployment-owned
//! credential registry (`USER_CORE_GRPC_SERVICE_CREDENTIALS`) — no request
//! signing, unlike its HTTP `/api/v1/users/me` family which is built for
//! browser-session delegation. That's the simplest available authenticated
//! path from a backend service, and the only one this client needs.

use std::time::Duration;

use tonic::client::Grpc;
use tonic::codec::ProstCodec;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;

const GET_USER_PATH: &str = "/user.v1.UserService/GetUser";
const GET_USER_PROFILE_PATH: &str = "/user.v1.UserService/GetUserProfile";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, PartialEq, ::prost::Message)]
struct GetUserRequest {
    #[prost(string, tag = "1")]
    id: String,
}

/// Only `email` (tag 2) of `user.v1.User`'s 9 fields — see the module doc.
#[derive(Clone, PartialEq, ::prost::Message)]
struct UserEmailOnly {
    #[prost(string, tag = "2")]
    email: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GetUserResponse {
    #[prost(message, optional, tag = "1")]
    user: Option<UserEmailOnly>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GetUserProfileRequest {
    #[prost(string, tag = "1")]
    user_id: String,
}

/// Only `phone` (tag 3) of `user.v1.UserProfile`'s 8 fields.
#[derive(Clone, PartialEq, ::prost::Message)]
struct ProfilePhoneOnly {
    #[prost(string, tag = "3")]
    phone: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GetUserProfileResponse {
    #[prost(message, optional, tag = "1")]
    profile: Option<ProfilePhoneOnly>,
}

/// A user's contact details, as far as user-core has them recorded. Both
/// fields are best-effort: an absent value means "user-core has none on
/// file," not an error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserContact {
    pub email: Option<String>,
    pub phone: Option<String>,
}

pub struct UserCoreClient {
    channel: Channel,
    credential_id: String,
    principal: String,
    service_auth: String,
}

impl UserCoreClient {
    /// Builds from `USER_CORE_GRPC_ADDR`/`USER_CORE_GRPC_CREDENTIAL_ID`/
    /// `USER_CORE_GRPC_PRINCIPAL`/`USER_CORE_GRPC_SERVICE_AUTH`. Returns
    /// `None` when any is missing — callers treat that as "contact lookup
    /// unavailable," never as a reason to fail the booking itself.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let addr = std::env::var("USER_CORE_GRPC_ADDR")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let credential_id = std::env::var("USER_CORE_GRPC_CREDENTIAL_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let principal = std::env::var("USER_CORE_GRPC_PRINCIPAL")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let service_auth = std::env::var("USER_CORE_GRPC_SERVICE_AUTH")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let channel = Endpoint::from_shared(addr)
            .ok()?
            .connect_timeout(CALL_TIMEOUT)
            .connect_lazy();
        Some(Self {
            channel,
            credential_id,
            principal,
            service_auth,
        })
    }

    fn authed_request<M>(&self, message: M) -> Result<Request<M>, String> {
        let mut request = Request::new(message);
        request.set_timeout(CALL_TIMEOUT);
        let metadata = request.metadata_mut();
        metadata.insert(
            "x-service-credential-id",
            MetadataValue::try_from(&self.credential_id)
                .map_err(|_| "user-core credential id is not forwardable".to_owned())?,
        );
        metadata.insert(
            "x-service-principal",
            MetadataValue::try_from(&self.principal)
                .map_err(|_| "user-core principal is not forwardable".to_owned())?,
        );
        metadata.insert(
            "x-service-auth",
            MetadataValue::try_from(&self.service_auth)
                .map_err(|_| "user-core service credential is not forwardable".to_owned())?,
        );
        Ok(request)
    }

    /// Looks up `user_id`'s contact details. Never returns `Err` for "not
    /// found" or "no profile" — those collapse to an empty `UserContact` so
    /// a caller can fall through to whatever it already had. `Err` is
    /// reserved for a genuine transport/auth failure worth logging.
    ///
    /// # Errors
    /// Returns an error string when the gRPC channel isn't ready, a
    /// metadata value can't be constructed, or either call fails at the
    /// transport/auth layer (not on a plain "not found").
    pub async fn get_contact(&self, user_id: &str) -> Result<UserContact, String> {
        let email = self.get_email(user_id).await?;
        let phone = self.get_phone(user_id).await?;
        Ok(UserContact { email, phone })
    }

    async fn get_email(&self, user_id: &str) -> Result<Option<String>, String> {
        let mut client = Grpc::new(self.channel.clone());
        client
            .ready()
            .await
            .map_err(|error| format!("user-core: channel not ready: {error}"))?;
        let request = self.authed_request(GetUserRequest {
            id: user_id.to_owned(),
        })?;
        let path = PathAndQuery::from_static(GET_USER_PATH);
        let response: tonic::Response<GetUserResponse> = match client
            .unary(request, path, ProstCodec::default())
            .await
        {
            Ok(response) => response,
            Err(status) if status.code() == tonic::Code::NotFound => return Ok(None),
            Err(status) => return Err(format!("user-core: GetUser failed: {status}")),
        };
        Ok(response
            .into_inner()
            .user
            .map(|user| user.email)
            .filter(|email| !email.trim().is_empty()))
    }

    async fn get_phone(&self, user_id: &str) -> Result<Option<String>, String> {
        let mut client = Grpc::new(self.channel.clone());
        client
            .ready()
            .await
            .map_err(|error| format!("user-core: channel not ready: {error}"))?;
        let request = self.authed_request(GetUserProfileRequest {
            user_id: user_id.to_owned(),
        })?;
        let path = PathAndQuery::from_static(GET_USER_PROFILE_PATH);
        let response: tonic::Response<GetUserProfileResponse> = match client
            .unary(request, path, ProstCodec::default())
            .await
        {
            Ok(response) => response,
            Err(status) if status.code() == tonic::Code::NotFound => return Ok(None),
            Err(status) => return Err(format!("user-core: GetUserProfile failed: {status}")),
        };
        Ok(response
            .into_inner()
            .profile
            .map(|profile| profile.phone)
            .filter(|phone| !phone.trim().is_empty()))
    }
}
