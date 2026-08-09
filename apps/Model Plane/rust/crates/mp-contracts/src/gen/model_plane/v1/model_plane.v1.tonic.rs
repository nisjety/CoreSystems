// @generated
/// Generated client implementations.
pub mod browser_broker_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct BrowserBrokerClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl BrowserBrokerClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> BrowserBrokerClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> BrowserBrokerClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            BrowserBrokerClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn acquire_grant(
            &mut self,
            request: impl tonic::IntoRequest<super::AcquireGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcquireGrantResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserBroker/AcquireGrant",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.BrowserBroker", "AcquireGrant"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn revoke_grant(
            &mut self,
            request: impl tonic::IntoRequest<super::RevokeGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RevokeGrantResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserBroker/RevokeGrant",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.BrowserBroker", "RevokeGrant"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn validate_grant(
            &mut self,
            request: impl tonic::IntoRequest<super::ValidateGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ValidateGrantResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserBroker/ValidateGrant",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.BrowserBroker", "ValidateGrant"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn health(
            &mut self,
            request: impl tonic::IntoRequest<super::BrowserHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BrowserHealthResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserBroker/Health",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.BrowserBroker", "Health"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod browser_broker_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with BrowserBrokerServer.
    #[async_trait]
    pub trait BrowserBroker: std::marker::Send + std::marker::Sync + 'static {
        async fn acquire_grant(
            &self,
            request: tonic::Request<super::AcquireGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcquireGrantResponse>,
            tonic::Status,
        >;
        async fn revoke_grant(
            &self,
            request: tonic::Request<super::RevokeGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RevokeGrantResponse>,
            tonic::Status,
        >;
        async fn validate_grant(
            &self,
            request: tonic::Request<super::ValidateGrantRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ValidateGrantResponse>,
            tonic::Status,
        >;
        async fn health(
            &self,
            request: tonic::Request<super::BrowserHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BrowserHealthResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct BrowserBrokerServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> BrowserBrokerServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for BrowserBrokerServer<T>
    where
        T: BrowserBroker,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.BrowserBroker/AcquireGrant" => {
                    #[allow(non_camel_case_types)]
                    struct AcquireGrantSvc<T: BrowserBroker>(pub Arc<T>);
                    impl<
                        T: BrowserBroker,
                    > tonic::server::UnaryService<super::AcquireGrantRequest>
                    for AcquireGrantSvc<T> {
                        type Response = super::AcquireGrantResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AcquireGrantRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserBroker>::acquire_grant(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AcquireGrantSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.BrowserBroker/RevokeGrant" => {
                    #[allow(non_camel_case_types)]
                    struct RevokeGrantSvc<T: BrowserBroker>(pub Arc<T>);
                    impl<
                        T: BrowserBroker,
                    > tonic::server::UnaryService<super::RevokeGrantRequest>
                    for RevokeGrantSvc<T> {
                        type Response = super::RevokeGrantResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RevokeGrantRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserBroker>::revoke_grant(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RevokeGrantSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.BrowserBroker/ValidateGrant" => {
                    #[allow(non_camel_case_types)]
                    struct ValidateGrantSvc<T: BrowserBroker>(pub Arc<T>);
                    impl<
                        T: BrowserBroker,
                    > tonic::server::UnaryService<super::ValidateGrantRequest>
                    for ValidateGrantSvc<T> {
                        type Response = super::ValidateGrantResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ValidateGrantRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserBroker>::validate_grant(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ValidateGrantSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.BrowserBroker/Health" => {
                    #[allow(non_camel_case_types)]
                    struct HealthSvc<T: BrowserBroker>(pub Arc<T>);
                    impl<
                        T: BrowserBroker,
                    > tonic::server::UnaryService<super::BrowserHealthRequest>
                    for HealthSvc<T> {
                        type Response = super::BrowserHealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BrowserHealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserBroker>::health(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = HealthSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for BrowserBrokerServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.BrowserBroker";
    impl<T> tonic::server::NamedService for BrowserBrokerServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod browser_agent_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct BrowserAgentServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl BrowserAgentServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> BrowserAgentServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> BrowserAgentServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            BrowserAgentServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn plan_action(
            &mut self,
            request: impl tonic::IntoRequest<super::PlanActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PlanActionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserAgentService/PlanAction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.BrowserAgentService", "PlanAction"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn report_observation(
            &mut self,
            request: impl tonic::IntoRequest<super::ReportObservationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReportObservationResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserAgentService/ReportObservation",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.BrowserAgentService",
                        "ReportObservation",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_agent_state(
            &mut self,
            request: impl tonic::IntoRequest<super::GetAgentStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetAgentStateResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.BrowserAgentService/GetAgentState",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.BrowserAgentService",
                        "GetAgentState",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod browser_agent_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with BrowserAgentServiceServer.
    #[async_trait]
    pub trait BrowserAgentService: std::marker::Send + std::marker::Sync + 'static {
        async fn plan_action(
            &self,
            request: tonic::Request<super::PlanActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PlanActionResponse>,
            tonic::Status,
        >;
        async fn report_observation(
            &self,
            request: tonic::Request<super::ReportObservationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReportObservationResponse>,
            tonic::Status,
        >;
        async fn get_agent_state(
            &self,
            request: tonic::Request<super::GetAgentStateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetAgentStateResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct BrowserAgentServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> BrowserAgentServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for BrowserAgentServiceServer<T>
    where
        T: BrowserAgentService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.BrowserAgentService/PlanAction" => {
                    #[allow(non_camel_case_types)]
                    struct PlanActionSvc<T: BrowserAgentService>(pub Arc<T>);
                    impl<
                        T: BrowserAgentService,
                    > tonic::server::UnaryService<super::PlanActionRequest>
                    for PlanActionSvc<T> {
                        type Response = super::PlanActionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PlanActionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserAgentService>::plan_action(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = PlanActionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.BrowserAgentService/ReportObservation" => {
                    #[allow(non_camel_case_types)]
                    struct ReportObservationSvc<T: BrowserAgentService>(pub Arc<T>);
                    impl<
                        T: BrowserAgentService,
                    > tonic::server::UnaryService<super::ReportObservationRequest>
                    for ReportObservationSvc<T> {
                        type Response = super::ReportObservationResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReportObservationRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserAgentService>::report_observation(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ReportObservationSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.BrowserAgentService/GetAgentState" => {
                    #[allow(non_camel_case_types)]
                    struct GetAgentStateSvc<T: BrowserAgentService>(pub Arc<T>);
                    impl<
                        T: BrowserAgentService,
                    > tonic::server::UnaryService<super::GetAgentStateRequest>
                    for GetAgentStateSvc<T> {
                        type Response = super::GetAgentStateResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetAgentStateRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as BrowserAgentService>::get_agent_state(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetAgentStateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for BrowserAgentServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.BrowserAgentService";
    impl<T> tonic::server::NamedService for BrowserAgentServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod capability_core_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct CapabilityCoreClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl CapabilityCoreClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> CapabilityCoreClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> CapabilityCoreClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            CapabilityCoreClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn list_capabilities(
            &mut self,
            request: impl tonic::IntoRequest<super::ListCapabilitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCapabilitiesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/ListCapabilities",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.CapabilityCore", "ListCapabilities"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_capability(
            &mut self,
            request: impl tonic::IntoRequest<super::GetCapabilityRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CapabilityDetail>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/GetCapability",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.CapabilityCore", "GetCapability"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn evaluate_policy(
            &mut self,
            request: impl tonic::IntoRequest<super::EvaluatePolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::EvaluatePolicyResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/EvaluatePolicy",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.CapabilityCore", "EvaluatePolicy"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn validate_skill_bundle(
            &mut self,
            request: impl tonic::IntoRequest<super::ValidateSkillBundleRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ValidateSkillBundleResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/ValidateSkillBundle",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.CapabilityCore",
                        "ValidateSkillBundle",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn check_skill_promotion(
            &mut self,
            request: impl tonic::IntoRequest<super::CheckSkillPromotionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckSkillPromotionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/CheckSkillPromotion",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.CapabilityCore",
                        "CheckSkillPromotion",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn promote_skill(
            &mut self,
            request: impl tonic::IntoRequest<super::PromoteSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PromoteSkillResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.CapabilityCore/PromoteSkill",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.CapabilityCore", "PromoteSkill"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod capability_core_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with CapabilityCoreServer.
    #[async_trait]
    pub trait CapabilityCore: std::marker::Send + std::marker::Sync + 'static {
        async fn list_capabilities(
            &self,
            request: tonic::Request<super::ListCapabilitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCapabilitiesResponse>,
            tonic::Status,
        >;
        async fn get_capability(
            &self,
            request: tonic::Request<super::GetCapabilityRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CapabilityDetail>,
            tonic::Status,
        >;
        async fn evaluate_policy(
            &self,
            request: tonic::Request<super::EvaluatePolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::EvaluatePolicyResponse>,
            tonic::Status,
        >;
        async fn validate_skill_bundle(
            &self,
            request: tonic::Request<super::ValidateSkillBundleRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ValidateSkillBundleResponse>,
            tonic::Status,
        >;
        async fn check_skill_promotion(
            &self,
            request: tonic::Request<super::CheckSkillPromotionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckSkillPromotionResponse>,
            tonic::Status,
        >;
        async fn promote_skill(
            &self,
            request: tonic::Request<super::PromoteSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PromoteSkillResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct CapabilityCoreServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> CapabilityCoreServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for CapabilityCoreServer<T>
    where
        T: CapabilityCore,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.CapabilityCore/ListCapabilities" => {
                    #[allow(non_camel_case_types)]
                    struct ListCapabilitiesSvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::ListCapabilitiesRequest>
                    for ListCapabilitiesSvc<T> {
                        type Response = super::ListCapabilitiesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListCapabilitiesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::list_capabilities(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListCapabilitiesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.CapabilityCore/GetCapability" => {
                    #[allow(non_camel_case_types)]
                    struct GetCapabilitySvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::GetCapabilityRequest>
                    for GetCapabilitySvc<T> {
                        type Response = super::CapabilityDetail;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetCapabilityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::get_capability(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetCapabilitySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.CapabilityCore/EvaluatePolicy" => {
                    #[allow(non_camel_case_types)]
                    struct EvaluatePolicySvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::EvaluatePolicyRequest>
                    for EvaluatePolicySvc<T> {
                        type Response = super::EvaluatePolicyResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::EvaluatePolicyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::evaluate_policy(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = EvaluatePolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.CapabilityCore/ValidateSkillBundle" => {
                    #[allow(non_camel_case_types)]
                    struct ValidateSkillBundleSvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::ValidateSkillBundleRequest>
                    for ValidateSkillBundleSvc<T> {
                        type Response = super::ValidateSkillBundleResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ValidateSkillBundleRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::validate_skill_bundle(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ValidateSkillBundleSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.CapabilityCore/CheckSkillPromotion" => {
                    #[allow(non_camel_case_types)]
                    struct CheckSkillPromotionSvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::CheckSkillPromotionRequest>
                    for CheckSkillPromotionSvc<T> {
                        type Response = super::CheckSkillPromotionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CheckSkillPromotionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::check_skill_promotion(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CheckSkillPromotionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.CapabilityCore/PromoteSkill" => {
                    #[allow(non_camel_case_types)]
                    struct PromoteSkillSvc<T: CapabilityCore>(pub Arc<T>);
                    impl<
                        T: CapabilityCore,
                    > tonic::server::UnaryService<super::PromoteSkillRequest>
                    for PromoteSkillSvc<T> {
                        type Response = super::PromoteSkillResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PromoteSkillRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as CapabilityCore>::promote_skill(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = PromoteSkillSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for CapabilityCoreServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.CapabilityCore";
    impl<T> tonic::server::NamedService for CapabilityCoreServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod event_log_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct EventLogClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl EventLogClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> EventLogClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> EventLogClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            EventLogClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn append_event(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.EventLog/AppendEvent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.EventLog", "AppendEvent"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn query_events(
            &mut self,
            request: impl tonic::IntoRequest<super::QueryEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::QueryEventsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.EventLog/QueryEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.EventLog", "QueryEvents"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn subscribe_events(
            &mut self,
            request: impl tonic::IntoRequest<super::SubscribeEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::Event>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.EventLog/SubscribeEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.EventLog", "SubscribeEvents"));
            self.inner.server_streaming(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod event_log_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with EventLogServer.
    #[async_trait]
    pub trait EventLog: std::marker::Send + std::marker::Sync + 'static {
        async fn append_event(
            &self,
            request: tonic::Request<super::AppendEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendEventResponse>,
            tonic::Status,
        >;
        async fn query_events(
            &self,
            request: tonic::Request<super::QueryEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::QueryEventsResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the SubscribeEvents method.
        type SubscribeEventsStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::Event, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        async fn subscribe_events(
            &self,
            request: tonic::Request<super::SubscribeEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::SubscribeEventsStream>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct EventLogServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> EventLogServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for EventLogServer<T>
    where
        T: EventLog,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.EventLog/AppendEvent" => {
                    #[allow(non_camel_case_types)]
                    struct AppendEventSvc<T: EventLog>(pub Arc<T>);
                    impl<
                        T: EventLog,
                    > tonic::server::UnaryService<super::AppendEventRequest>
                    for AppendEventSvc<T> {
                        type Response = super::AppendEventResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendEventRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EventLog>::append_event(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AppendEventSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.EventLog/QueryEvents" => {
                    #[allow(non_camel_case_types)]
                    struct QueryEventsSvc<T: EventLog>(pub Arc<T>);
                    impl<
                        T: EventLog,
                    > tonic::server::UnaryService<super::QueryEventsRequest>
                    for QueryEventsSvc<T> {
                        type Response = super::QueryEventsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::QueryEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EventLog>::query_events(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = QueryEventsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.EventLog/SubscribeEvents" => {
                    #[allow(non_camel_case_types)]
                    struct SubscribeEventsSvc<T: EventLog>(pub Arc<T>);
                    impl<
                        T: EventLog,
                    > tonic::server::ServerStreamingService<
                        super::SubscribeEventsRequest,
                    > for SubscribeEventsSvc<T> {
                        type Response = super::Event;
                        type ResponseStream = T::SubscribeEventsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SubscribeEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EventLog>::subscribe_events(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SubscribeEventsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for EventLogServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.EventLog";
    impl<T> tonic::server::NamedService for EventLogServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod run_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct RunServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl RunServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> RunServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> RunServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            RunServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn get_run(
            &mut self,
            request: impl tonic::IntoRequest<super::GetRunRequest>,
        ) -> std::result::Result<tonic::Response<super::RunDetail>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RunService/GetRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RunService", "GetRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_runs(
            &mut self,
            request: impl tonic::IntoRequest<super::ListRunsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListRunsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RunService/ListRuns",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RunService", "ListRuns"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn cancel_run(
            &mut self,
            request: impl tonic::IntoRequest<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RunService/CancelRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RunService", "CancelRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_system_runs(
            &mut self,
            request: impl tonic::IntoRequest<super::ListSystemRunsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListRunsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RunService/ListSystemRuns",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RunService", "ListSystemRuns"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn resolve_run_owner(
            &mut self,
            request: impl tonic::IntoRequest<super::ResolveRunOwnerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResolveRunOwnerResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RunService/ResolveRunOwner",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RunService", "ResolveRunOwner"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod run_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with RunServiceServer.
    #[async_trait]
    pub trait RunService: std::marker::Send + std::marker::Sync + 'static {
        async fn get_run(
            &self,
            request: tonic::Request<super::GetRunRequest>,
        ) -> std::result::Result<tonic::Response<super::RunDetail>, tonic::Status>;
        async fn list_runs(
            &self,
            request: tonic::Request<super::ListRunsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListRunsResponse>,
            tonic::Status,
        >;
        async fn cancel_run(
            &self,
            request: tonic::Request<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        >;
        async fn list_system_runs(
            &self,
            request: tonic::Request<super::ListSystemRunsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListRunsResponse>,
            tonic::Status,
        >;
        async fn resolve_run_owner(
            &self,
            request: tonic::Request<super::ResolveRunOwnerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResolveRunOwnerResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct RunServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> RunServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for RunServiceServer<T>
    where
        T: RunService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.RunService/GetRun" => {
                    #[allow(non_camel_case_types)]
                    struct GetRunSvc<T: RunService>(pub Arc<T>);
                    impl<T: RunService> tonic::server::UnaryService<super::GetRunRequest>
                    for GetRunSvc<T> {
                        type Response = super::RunDetail;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RunService>::get_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.RunService/ListRuns" => {
                    #[allow(non_camel_case_types)]
                    struct ListRunsSvc<T: RunService>(pub Arc<T>);
                    impl<
                        T: RunService,
                    > tonic::server::UnaryService<super::ListRunsRequest>
                    for ListRunsSvc<T> {
                        type Response = super::ListRunsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListRunsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RunService>::list_runs(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListRunsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.RunService/CancelRun" => {
                    #[allow(non_camel_case_types)]
                    struct CancelRunSvc<T: RunService>(pub Arc<T>);
                    impl<
                        T: RunService,
                    > tonic::server::UnaryService<super::CancelRunRequest>
                    for CancelRunSvc<T> {
                        type Response = super::CancelRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CancelRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RunService>::cancel_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CancelRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.RunService/ListSystemRuns" => {
                    #[allow(non_camel_case_types)]
                    struct ListSystemRunsSvc<T: RunService>(pub Arc<T>);
                    impl<
                        T: RunService,
                    > tonic::server::UnaryService<super::ListSystemRunsRequest>
                    for ListSystemRunsSvc<T> {
                        type Response = super::ListRunsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListSystemRunsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RunService>::list_system_runs(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListSystemRunsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.RunService/ResolveRunOwner" => {
                    #[allow(non_camel_case_types)]
                    struct ResolveRunOwnerSvc<T: RunService>(pub Arc<T>);
                    impl<
                        T: RunService,
                    > tonic::server::UnaryService<super::ResolveRunOwnerRequest>
                    for ResolveRunOwnerSvc<T> {
                        type Response = super::ResolveRunOwnerResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ResolveRunOwnerRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RunService>::resolve_run_owner(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ResolveRunOwnerSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for RunServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.RunService";
    impl<T> tonic::server::NamedService for RunServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod inference_core_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct InferenceCoreClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl InferenceCoreClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> InferenceCoreClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InferenceCoreClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            InferenceCoreClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn infer(
            &mut self,
            request: impl tonic::IntoRequest<super::InferRequest>,
        ) -> std::result::Result<tonic::Response<super::InferResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/Infer",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.InferenceCore", "Infer"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn infer_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::InferRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::InferChunk>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/InferStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.InferenceCore", "InferStream"));
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn create_embedding(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateEmbeddingRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateEmbeddingResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/CreateEmbedding",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "CreateEmbedding"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_models(
            &mut self,
            request: impl tonic::IntoRequest<super::ListModelsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListModelsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/ListModels",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.InferenceCore", "ListModels"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn synthesize_speech(
            &mut self,
            request: impl tonic::IntoRequest<super::SynthesizeSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SynthesizeSpeechResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/SynthesizeSpeech",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "SynthesizeSpeech"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn transcribe_speech(
            &mut self,
            request: impl tonic::IntoRequest<super::TranscribeSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TranscribeSpeechResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/TranscribeSpeech",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "TranscribeSpeech"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_speech_voices(
            &mut self,
            request: impl tonic::IntoRequest<super::ListSpeechVoicesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSpeechVoicesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/ListSpeechVoices",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "ListSpeechVoices"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn translate_text(
            &mut self,
            request: impl tonic::IntoRequest<super::TranslateTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TranslateTextResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/TranslateText",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "TranslateText"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn batch_translate_text(
            &mut self,
            request: impl tonic::IntoRequest<super::BatchTranslateTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchTranslateTextResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/BatchTranslateText",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "BatchTranslateText"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn detect_text_language(
            &mut self,
            request: impl tonic::IntoRequest<super::DetectTextLanguageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DetectTextLanguageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/DetectTextLanguage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "DetectTextLanguage"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_translation_languages(
            &mut self,
            request: impl tonic::IntoRequest<super::ListTranslationLanguagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTranslationLanguagesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/ListTranslationLanguages",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.InferenceCore",
                        "ListTranslationLanguages",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn generate_image(
            &mut self,
            request: impl tonic::IntoRequest<super::GenerateImageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GenerateImageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/GenerateImage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "GenerateImage"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn analyze_image(
            &mut self,
            request: impl tonic::IntoRequest<super::AnalyzeImageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeImageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/AnalyzeImage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.InferenceCore", "AnalyzeImage"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn extract_image_text(
            &mut self,
            request: impl tonic::IntoRequest<super::ExtractImageTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExtractImageTextResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/ExtractImageText",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "ExtractImageText"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn analyze_document(
            &mut self,
            request: impl tonic::IntoRequest<super::AnalyzeDocumentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeDocumentResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/AnalyzeDocument",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "AnalyzeDocument"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn analyze_language(
            &mut self,
            request: impl tonic::IntoRequest<super::AnalyzeLanguageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeLanguageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/AnalyzeLanguage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.InferenceCore", "AnalyzeLanguage"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn create_realtime_session(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateRealtimeSessionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateRealtimeSessionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/CreateRealtimeSession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.InferenceCore",
                        "CreateRealtimeSession",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn create_video_generation_job(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateVideoGenerationJobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateVideoGenerationJobResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/CreateVideoGenerationJob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.InferenceCore",
                        "CreateVideoGenerationJob",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_video_generation_job(
            &mut self,
            request: impl tonic::IntoRequest<super::GetVideoGenerationJobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVideoGenerationJobResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/GetVideoGenerationJob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.InferenceCore",
                        "GetVideoGenerationJob",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn stream_video_generation_content(
            &mut self,
            request: impl tonic::IntoRequest<super::StreamVideoGenerationContentRequest>,
        ) -> std::result::Result<
            tonic::Response<
                tonic::codec::Streaming<super::StreamVideoGenerationContentResponse>,
            >,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.InferenceCore/StreamVideoGenerationContent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.InferenceCore",
                        "StreamVideoGenerationContent",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod inference_core_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with InferenceCoreServer.
    #[async_trait]
    pub trait InferenceCore: std::marker::Send + std::marker::Sync + 'static {
        async fn infer(
            &self,
            request: tonic::Request<super::InferRequest>,
        ) -> std::result::Result<tonic::Response<super::InferResponse>, tonic::Status>;
        /// Server streaming response type for the InferStream method.
        type InferStreamStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::InferChunk, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        async fn infer_stream(
            &self,
            request: tonic::Request<super::InferRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::InferStreamStream>,
            tonic::Status,
        >;
        async fn create_embedding(
            &self,
            request: tonic::Request<super::CreateEmbeddingRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateEmbeddingResponse>,
            tonic::Status,
        >;
        async fn list_models(
            &self,
            request: tonic::Request<super::ListModelsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListModelsResponse>,
            tonic::Status,
        >;
        async fn synthesize_speech(
            &self,
            request: tonic::Request<super::SynthesizeSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SynthesizeSpeechResponse>,
            tonic::Status,
        >;
        async fn transcribe_speech(
            &self,
            request: tonic::Request<super::TranscribeSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TranscribeSpeechResponse>,
            tonic::Status,
        >;
        async fn list_speech_voices(
            &self,
            request: tonic::Request<super::ListSpeechVoicesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSpeechVoicesResponse>,
            tonic::Status,
        >;
        async fn translate_text(
            &self,
            request: tonic::Request<super::TranslateTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TranslateTextResponse>,
            tonic::Status,
        >;
        async fn batch_translate_text(
            &self,
            request: tonic::Request<super::BatchTranslateTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchTranslateTextResponse>,
            tonic::Status,
        >;
        async fn detect_text_language(
            &self,
            request: tonic::Request<super::DetectTextLanguageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DetectTextLanguageResponse>,
            tonic::Status,
        >;
        async fn list_translation_languages(
            &self,
            request: tonic::Request<super::ListTranslationLanguagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTranslationLanguagesResponse>,
            tonic::Status,
        >;
        async fn generate_image(
            &self,
            request: tonic::Request<super::GenerateImageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GenerateImageResponse>,
            tonic::Status,
        >;
        async fn analyze_image(
            &self,
            request: tonic::Request<super::AnalyzeImageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeImageResponse>,
            tonic::Status,
        >;
        async fn extract_image_text(
            &self,
            request: tonic::Request<super::ExtractImageTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExtractImageTextResponse>,
            tonic::Status,
        >;
        async fn analyze_document(
            &self,
            request: tonic::Request<super::AnalyzeDocumentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeDocumentResponse>,
            tonic::Status,
        >;
        async fn analyze_language(
            &self,
            request: tonic::Request<super::AnalyzeLanguageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AnalyzeLanguageResponse>,
            tonic::Status,
        >;
        async fn create_realtime_session(
            &self,
            request: tonic::Request<super::CreateRealtimeSessionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateRealtimeSessionResponse>,
            tonic::Status,
        >;
        async fn create_video_generation_job(
            &self,
            request: tonic::Request<super::CreateVideoGenerationJobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateVideoGenerationJobResponse>,
            tonic::Status,
        >;
        async fn get_video_generation_job(
            &self,
            request: tonic::Request<super::GetVideoGenerationJobRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetVideoGenerationJobResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the StreamVideoGenerationContent method.
        type StreamVideoGenerationContentStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<
                    super::StreamVideoGenerationContentResponse,
                    tonic::Status,
                >,
            >
            + std::marker::Send
            + 'static;
        async fn stream_video_generation_content(
            &self,
            request: tonic::Request<super::StreamVideoGenerationContentRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::StreamVideoGenerationContentStream>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct InferenceCoreServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> InferenceCoreServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for InferenceCoreServer<T>
    where
        T: InferenceCore,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.InferenceCore/Infer" => {
                    #[allow(non_camel_case_types)]
                    struct InferSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::InferRequest> for InferSvc<T> {
                        type Response = super::InferResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InferRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::infer(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = InferSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/InferStream" => {
                    #[allow(non_camel_case_types)]
                    struct InferStreamSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::ServerStreamingService<super::InferRequest>
                    for InferStreamSvc<T> {
                        type Response = super::InferChunk;
                        type ResponseStream = T::InferStreamStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InferRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::infer_stream(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = InferStreamSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/CreateEmbedding" => {
                    #[allow(non_camel_case_types)]
                    struct CreateEmbeddingSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::CreateEmbeddingRequest>
                    for CreateEmbeddingSvc<T> {
                        type Response = super::CreateEmbeddingResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateEmbeddingRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::create_embedding(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateEmbeddingSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/ListModels" => {
                    #[allow(non_camel_case_types)]
                    struct ListModelsSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::ListModelsRequest>
                    for ListModelsSvc<T> {
                        type Response = super::ListModelsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListModelsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::list_models(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListModelsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/SynthesizeSpeech" => {
                    #[allow(non_camel_case_types)]
                    struct SynthesizeSpeechSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::SynthesizeSpeechRequest>
                    for SynthesizeSpeechSvc<T> {
                        type Response = super::SynthesizeSpeechResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SynthesizeSpeechRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::synthesize_speech(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SynthesizeSpeechSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/TranscribeSpeech" => {
                    #[allow(non_camel_case_types)]
                    struct TranscribeSpeechSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::TranscribeSpeechRequest>
                    for TranscribeSpeechSvc<T> {
                        type Response = super::TranscribeSpeechResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TranscribeSpeechRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::transcribe_speech(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TranscribeSpeechSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/ListSpeechVoices" => {
                    #[allow(non_camel_case_types)]
                    struct ListSpeechVoicesSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::ListSpeechVoicesRequest>
                    for ListSpeechVoicesSvc<T> {
                        type Response = super::ListSpeechVoicesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListSpeechVoicesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::list_speech_voices(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListSpeechVoicesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/TranslateText" => {
                    #[allow(non_camel_case_types)]
                    struct TranslateTextSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::TranslateTextRequest>
                    for TranslateTextSvc<T> {
                        type Response = super::TranslateTextResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TranslateTextRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::translate_text(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TranslateTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/BatchTranslateText" => {
                    #[allow(non_camel_case_types)]
                    struct BatchTranslateTextSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::BatchTranslateTextRequest>
                    for BatchTranslateTextSvc<T> {
                        type Response = super::BatchTranslateTextResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BatchTranslateTextRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::batch_translate_text(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = BatchTranslateTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/DetectTextLanguage" => {
                    #[allow(non_camel_case_types)]
                    struct DetectTextLanguageSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::DetectTextLanguageRequest>
                    for DetectTextLanguageSvc<T> {
                        type Response = super::DetectTextLanguageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DetectTextLanguageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::detect_text_language(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DetectTextLanguageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/ListTranslationLanguages" => {
                    #[allow(non_camel_case_types)]
                    struct ListTranslationLanguagesSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::ListTranslationLanguagesRequest>
                    for ListTranslationLanguagesSvc<T> {
                        type Response = super::ListTranslationLanguagesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::ListTranslationLanguagesRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::list_translation_languages(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListTranslationLanguagesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/GenerateImage" => {
                    #[allow(non_camel_case_types)]
                    struct GenerateImageSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::GenerateImageRequest>
                    for GenerateImageSvc<T> {
                        type Response = super::GenerateImageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GenerateImageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::generate_image(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GenerateImageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/AnalyzeImage" => {
                    #[allow(non_camel_case_types)]
                    struct AnalyzeImageSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::AnalyzeImageRequest>
                    for AnalyzeImageSvc<T> {
                        type Response = super::AnalyzeImageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AnalyzeImageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::analyze_image(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AnalyzeImageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/ExtractImageText" => {
                    #[allow(non_camel_case_types)]
                    struct ExtractImageTextSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::ExtractImageTextRequest>
                    for ExtractImageTextSvc<T> {
                        type Response = super::ExtractImageTextResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExtractImageTextRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::extract_image_text(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExtractImageTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/AnalyzeDocument" => {
                    #[allow(non_camel_case_types)]
                    struct AnalyzeDocumentSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::AnalyzeDocumentRequest>
                    for AnalyzeDocumentSvc<T> {
                        type Response = super::AnalyzeDocumentResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AnalyzeDocumentRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::analyze_document(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AnalyzeDocumentSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/AnalyzeLanguage" => {
                    #[allow(non_camel_case_types)]
                    struct AnalyzeLanguageSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::AnalyzeLanguageRequest>
                    for AnalyzeLanguageSvc<T> {
                        type Response = super::AnalyzeLanguageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AnalyzeLanguageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::analyze_language(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AnalyzeLanguageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/CreateRealtimeSession" => {
                    #[allow(non_camel_case_types)]
                    struct CreateRealtimeSessionSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::CreateRealtimeSessionRequest>
                    for CreateRealtimeSessionSvc<T> {
                        type Response = super::CreateRealtimeSessionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateRealtimeSessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::create_realtime_session(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateRealtimeSessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/CreateVideoGenerationJob" => {
                    #[allow(non_camel_case_types)]
                    struct CreateVideoGenerationJobSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::CreateVideoGenerationJobRequest>
                    for CreateVideoGenerationJobSvc<T> {
                        type Response = super::CreateVideoGenerationJobResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::CreateVideoGenerationJobRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::create_video_generation_job(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateVideoGenerationJobSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/GetVideoGenerationJob" => {
                    #[allow(non_camel_case_types)]
                    struct GetVideoGenerationJobSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::UnaryService<super::GetVideoGenerationJobRequest>
                    for GetVideoGenerationJobSvc<T> {
                        type Response = super::GetVideoGenerationJobResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetVideoGenerationJobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::get_video_generation_job(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetVideoGenerationJobSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.InferenceCore/StreamVideoGenerationContent" => {
                    #[allow(non_camel_case_types)]
                    struct StreamVideoGenerationContentSvc<T: InferenceCore>(pub Arc<T>);
                    impl<
                        T: InferenceCore,
                    > tonic::server::ServerStreamingService<
                        super::StreamVideoGenerationContentRequest,
                    > for StreamVideoGenerationContentSvc<T> {
                        type Response = super::StreamVideoGenerationContentResponse;
                        type ResponseStream = T::StreamVideoGenerationContentStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::StreamVideoGenerationContentRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as InferenceCore>::stream_video_generation_content(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StreamVideoGenerationContentSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for InferenceCoreServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.InferenceCore";
    impl<T> tonic::server::NamedService for InferenceCoreServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod execution_core_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct ExecutionCoreClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ExecutionCoreClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> ExecutionCoreClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ExecutionCoreClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            ExecutionCoreClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn execute_step(
            &mut self,
            request: impl tonic::IntoRequest<super::ExecuteStepRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteStepResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ExecutionCore/ExecuteStep",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ExecutionCore", "ExecuteStep"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn resume_run(
            &mut self,
            request: impl tonic::IntoRequest<super::ResumeRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResumeRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ExecutionCore/ResumeRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ExecutionCore", "ResumeRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn cancel_run(
            &mut self,
            request: impl tonic::IntoRequest<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ExecutionCore/CancelRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ExecutionCore", "CancelRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn pause_run(
            &mut self,
            request: impl tonic::IntoRequest<super::PauseRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PauseRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ExecutionCore/PauseRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ExecutionCore", "PauseRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn run_agent(
            &mut self,
            request: impl tonic::IntoRequest<super::RunAgentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RunAgentResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ExecutionCore/RunAgent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ExecutionCore", "RunAgent"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod execution_core_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ExecutionCoreServer.
    #[async_trait]
    pub trait ExecutionCore: std::marker::Send + std::marker::Sync + 'static {
        async fn execute_step(
            &self,
            request: tonic::Request<super::ExecuteStepRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteStepResponse>,
            tonic::Status,
        >;
        async fn resume_run(
            &self,
            request: tonic::Request<super::ResumeRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ResumeRunResponse>,
            tonic::Status,
        >;
        async fn cancel_run(
            &self,
            request: tonic::Request<super::CancelRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CancelRunResponse>,
            tonic::Status,
        >;
        async fn pause_run(
            &self,
            request: tonic::Request<super::PauseRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::PauseRunResponse>,
            tonic::Status,
        >;
        async fn run_agent(
            &self,
            request: tonic::Request<super::RunAgentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RunAgentResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct ExecutionCoreServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> ExecutionCoreServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ExecutionCoreServer<T>
    where
        T: ExecutionCore,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.ExecutionCore/ExecuteStep" => {
                    #[allow(non_camel_case_types)]
                    struct ExecuteStepSvc<T: ExecutionCore>(pub Arc<T>);
                    impl<
                        T: ExecutionCore,
                    > tonic::server::UnaryService<super::ExecuteStepRequest>
                    for ExecuteStepSvc<T> {
                        type Response = super::ExecuteStepResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExecuteStepRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ExecutionCore>::execute_step(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExecuteStepSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ExecutionCore/ResumeRun" => {
                    #[allow(non_camel_case_types)]
                    struct ResumeRunSvc<T: ExecutionCore>(pub Arc<T>);
                    impl<
                        T: ExecutionCore,
                    > tonic::server::UnaryService<super::ResumeRunRequest>
                    for ResumeRunSvc<T> {
                        type Response = super::ResumeRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ResumeRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ExecutionCore>::resume_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ResumeRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ExecutionCore/CancelRun" => {
                    #[allow(non_camel_case_types)]
                    struct CancelRunSvc<T: ExecutionCore>(pub Arc<T>);
                    impl<
                        T: ExecutionCore,
                    > tonic::server::UnaryService<super::CancelRunRequest>
                    for CancelRunSvc<T> {
                        type Response = super::CancelRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CancelRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ExecutionCore>::cancel_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CancelRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ExecutionCore/PauseRun" => {
                    #[allow(non_camel_case_types)]
                    struct PauseRunSvc<T: ExecutionCore>(pub Arc<T>);
                    impl<
                        T: ExecutionCore,
                    > tonic::server::UnaryService<super::PauseRunRequest>
                    for PauseRunSvc<T> {
                        type Response = super::PauseRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PauseRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ExecutionCore>::pause_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = PauseRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ExecutionCore/RunAgent" => {
                    #[allow(non_camel_case_types)]
                    struct RunAgentSvc<T: ExecutionCore>(pub Arc<T>);
                    impl<
                        T: ExecutionCore,
                    > tonic::server::UnaryService<super::RunAgentRequest>
                    for RunAgentSvc<T> {
                        type Response = super::RunAgentResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RunAgentRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ExecutionCore>::run_agent(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RunAgentSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for ExecutionCoreServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.ExecutionCore";
    impl<T> tonic::server::NamedService for ExecutionCoreServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod finetune_jobs_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct FinetuneJobsClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl FinetuneJobsClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> FinetuneJobsClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> FinetuneJobsClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            FinetuneJobsClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn create_job(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateFinetuneJobRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/CreateJob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.FinetuneJobs", "CreateJob"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_job(
            &mut self,
            request: impl tonic::IntoRequest<super::GetFinetuneJobRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/GetJob",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.FinetuneJobs", "GetJob"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_jobs(
            &mut self,
            request: impl tonic::IntoRequest<super::ListFinetuneJobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListFinetuneJobsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/ListJobs",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.FinetuneJobs", "ListJobs"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn update_job_status(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateFinetuneJobStatusRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/UpdateJobStatus",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.FinetuneJobs", "UpdateJobStatus"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_active_jobs(
            &mut self,
            request: impl tonic::IntoRequest<super::ListActiveFinetuneJobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListActiveFinetuneJobsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/ListActiveJobs",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.FinetuneJobs", "ListActiveJobs"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_org_monthly_spend(
            &mut self,
            request: impl tonic::IntoRequest<super::GetOrgMonthlySpendRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrgMonthlySpendResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.FinetuneJobs/GetOrgMonthlySpend",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.FinetuneJobs", "GetOrgMonthlySpend"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod finetune_jobs_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with FinetuneJobsServer.
    #[async_trait]
    pub trait FinetuneJobs: std::marker::Send + std::marker::Sync + 'static {
        async fn create_job(
            &self,
            request: tonic::Request<super::CreateFinetuneJobRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status>;
        async fn get_job(
            &self,
            request: tonic::Request<super::GetFinetuneJobRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status>;
        async fn list_jobs(
            &self,
            request: tonic::Request<super::ListFinetuneJobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListFinetuneJobsResponse>,
            tonic::Status,
        >;
        async fn update_job_status(
            &self,
            request: tonic::Request<super::UpdateFinetuneJobStatusRequest>,
        ) -> std::result::Result<tonic::Response<super::FinetuneJob>, tonic::Status>;
        async fn list_active_jobs(
            &self,
            request: tonic::Request<super::ListActiveFinetuneJobsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListActiveFinetuneJobsResponse>,
            tonic::Status,
        >;
        async fn get_org_monthly_spend(
            &self,
            request: tonic::Request<super::GetOrgMonthlySpendRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetOrgMonthlySpendResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct FinetuneJobsServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> FinetuneJobsServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for FinetuneJobsServer<T>
    where
        T: FinetuneJobs,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.FinetuneJobs/CreateJob" => {
                    #[allow(non_camel_case_types)]
                    struct CreateJobSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::CreateFinetuneJobRequest>
                    for CreateJobSvc<T> {
                        type Response = super::FinetuneJob;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateFinetuneJobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::create_job(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateJobSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.FinetuneJobs/GetJob" => {
                    #[allow(non_camel_case_types)]
                    struct GetJobSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::GetFinetuneJobRequest>
                    for GetJobSvc<T> {
                        type Response = super::FinetuneJob;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetFinetuneJobRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::get_job(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetJobSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.FinetuneJobs/ListJobs" => {
                    #[allow(non_camel_case_types)]
                    struct ListJobsSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::ListFinetuneJobsRequest>
                    for ListJobsSvc<T> {
                        type Response = super::ListFinetuneJobsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListFinetuneJobsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::list_jobs(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListJobsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.FinetuneJobs/UpdateJobStatus" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateJobStatusSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::UpdateFinetuneJobStatusRequest>
                    for UpdateJobStatusSvc<T> {
                        type Response = super::FinetuneJob;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::UpdateFinetuneJobStatusRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::update_job_status(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = UpdateJobStatusSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.FinetuneJobs/ListActiveJobs" => {
                    #[allow(non_camel_case_types)]
                    struct ListActiveJobsSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::ListActiveFinetuneJobsRequest>
                    for ListActiveJobsSvc<T> {
                        type Response = super::ListActiveFinetuneJobsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListActiveFinetuneJobsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::list_active_jobs(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListActiveJobsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.FinetuneJobs/GetOrgMonthlySpend" => {
                    #[allow(non_camel_case_types)]
                    struct GetOrgMonthlySpendSvc<T: FinetuneJobs>(pub Arc<T>);
                    impl<
                        T: FinetuneJobs,
                    > tonic::server::UnaryService<super::GetOrgMonthlySpendRequest>
                    for GetOrgMonthlySpendSvc<T> {
                        type Response = super::GetOrgMonthlySpendResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetOrgMonthlySpendRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as FinetuneJobs>::get_org_monthly_spend(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetOrgMonthlySpendSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for FinetuneJobsServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.FinetuneJobs";
    impl<T> tonic::server::NamedService for FinetuneJobsServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod model_gateway_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct ModelGatewayClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ModelGatewayClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> ModelGatewayClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ModelGatewayClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            ModelGatewayClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn invoke(
            &mut self,
            request: impl tonic::IntoRequest<super::InvokeRequest>,
        ) -> std::result::Result<tonic::Response<super::InvokeResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/Invoke",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "Invoke"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn invoke_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::InvokeRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::InvokeChunk>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/InvokeStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "InvokeStream"));
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn fetch(
            &mut self,
            request: impl tonic::IntoRequest<super::FetchRequest>,
        ) -> std::result::Result<tonic::Response<super::FetchResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/Fetch",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "Fetch"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn extract_structured(
            &mut self,
            request: impl tonic::IntoRequest<super::ExtractStructuredRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExtractStructuredResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ExtractStructured",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ExtractStructured"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn web_search(
            &mut self,
            request: impl tonic::IntoRequest<super::WebSearchRequest>,
        ) -> std::result::Result<
            tonic::Response<super::WebSearchResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/WebSearch",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "WebSearch"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn sleep(
            &mut self,
            request: impl tonic::IntoRequest<super::SleepRequest>,
        ) -> std::result::Result<tonic::Response<super::SleepResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/Sleep",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "Sleep"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn remote_trigger(
            &mut self,
            request: impl tonic::IntoRequest<super::RemoteTriggerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RemoteTriggerResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RemoteTrigger",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "RemoteTrigger"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn send_message(
            &mut self,
            request: impl tonic::IntoRequest<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SendMessage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "SendMessage"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn synthetic_output(
            &mut self,
            request: impl tonic::IntoRequest<super::SyntheticOutputRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SyntheticOutputResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SyntheticOutput",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "SyntheticOutput"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn enter_plan_mode(
            &mut self,
            request: impl tonic::IntoRequest<super::EnterPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::EnterPlanModeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/EnterPlanMode",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "EnterPlanMode"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn exit_plan_mode(
            &mut self,
            request: impl tonic::IntoRequest<super::ExitPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExitPlanModeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ExitPlanMode",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ExitPlanMode"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn is_plan_mode(
            &mut self,
            request: impl tonic::IntoRequest<super::IsPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::IsPlanModeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/IsPlanMode",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "IsPlanMode"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn team_create(
            &mut self,
            request: impl tonic::IntoRequest<super::TeamCreateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamCreateResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/TeamCreate",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "TeamCreate"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn team_delete(
            &mut self,
            request: impl tonic::IntoRequest<super::TeamDeleteRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamDeleteResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/TeamDelete",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "TeamDelete"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn team_list(
            &mut self,
            request: impl tonic::IntoRequest<super::TeamListRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamListResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/TeamList",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "TeamList"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn lsp_query(
            &mut self,
            request: impl tonic::IntoRequest<super::LspQueryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LspQueryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/LspQuery",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "LspQuery"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn request_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::RequestApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RequestApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RequestApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "RequestApproval"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn approve_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::ApproveApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ApproveApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ApproveApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ApproveApproval"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn deny_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::DenyApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DenyApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/DenyApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "DenyApproval"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_pending_approvals(
            &mut self,
            request: impl tonic::IntoRequest<super::ListPendingApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPendingApprovalsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListPendingApprovals",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.ModelGateway",
                        "ListPendingApprovals",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn record_trajectory(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordTrajectoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordTrajectoryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RecordTrajectory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "RecordTrajectory"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_trajectories(
            &mut self,
            request: impl tonic::IntoRequest<super::ListTrajectoriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTrajectoriesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListTrajectories",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ListTrajectories"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn export_trajectories(
            &mut self,
            request: impl tonic::IntoRequest<super::ExportTrajectoriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExportTrajectoriesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ExportTrajectories",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ExportTrajectories"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_skills(
            &mut self,
            request: impl tonic::IntoRequest<super::ListSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSkillsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListSkills",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListSkills"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_skill(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSkillResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/GetSkill",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "GetSkill"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn match_skills(
            &mut self,
            request: impl tonic::IntoRequest<super::MatchSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MatchSkillsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/MatchSkills",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "MatchSkills"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn register_mcp_server(
            &mut self,
            request: impl tonic::IntoRequest<super::RegisterMcpServerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterMcpServerResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RegisterMcpServer",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "RegisterMcpServer"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_mcp_servers(
            &mut self,
            request: impl tonic::IntoRequest<super::ListMcpServersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMcpServersResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListMcpServers",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ListMcpServers"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn proxy_mcp_tool(
            &mut self,
            request: impl tonic::IntoRequest<super::ProxyMcpToolRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ProxyMcpToolResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ProxyMcpTool",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ProxyMcpTool"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_mcp_tools(
            &mut self,
            request: impl tonic::IntoRequest<super::ListMcpToolsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMcpToolsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListMcpTools",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListMcpTools"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn register_plugin(
            &mut self,
            request: impl tonic::IntoRequest<super::RegisterPluginRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterPluginResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RegisterPlugin",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "RegisterPlugin"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_plugins(
            &mut self,
            request: impl tonic::IntoRequest<super::ListPluginsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPluginsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListPlugins",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListPlugins"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_plugin_enabled(
            &mut self,
            request: impl tonic::IntoRequest<super::SetPluginEnabledRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPluginEnabledResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SetPluginEnabled",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "SetPluginEnabled"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_commands(
            &mut self,
            request: impl tonic::IntoRequest<super::ListCommandsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCommandsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListCommands",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListCommands"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn execute_command(
            &mut self,
            request: impl tonic::IntoRequest<super::ExecuteCommandRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteCommandResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ExecuteCommand",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ExecuteCommand"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn register_hook(
            &mut self,
            request: impl tonic::IntoRequest<super::RegisterHookRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterHookResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/RegisterHook",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "RegisterHook"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_hooks(
            &mut self,
            request: impl tonic::IntoRequest<super::ListHooksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListHooksResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListHooks",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListHooks"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn check_permission(
            &mut self,
            request: impl tonic::IntoRequest<super::CheckPermissionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckPermissionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/CheckPermission",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "CheckPermission"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_permission(
            &mut self,
            request: impl tonic::IntoRequest<super::SetPermissionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPermissionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SetPermission",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "SetPermission"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_policy(
            &mut self,
            request: impl tonic::IntoRequest<super::GetPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPolicyResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/GetPolicy",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "GetPolicy"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_policy(
            &mut self,
            request: impl tonic::IntoRequest<super::SetPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPolicyResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SetPolicy",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "SetPolicy"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn append_thread_message(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendThreadMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendThreadMessageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/AppendThreadMessage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "AppendThreadMessage"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_thread_messages(
            &mut self,
            request: impl tonic::IntoRequest<super::ListThreadMessagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadMessagesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListThreadMessages",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.ModelGateway", "ListThreadMessages"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_analytics(
            &mut self,
            request: impl tonic::IntoRequest<super::GetAnalyticsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetAnalyticsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/GetAnalytics",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "GetAnalytics"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn text_to_speech(
            &mut self,
            request: impl tonic::IntoRequest<super::TextToSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TextToSpeechResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/TextToSpeech",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "TextToSpeech"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn speech_to_text(
            &mut self,
            request: impl tonic::IntoRequest<super::SpeechToTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpeechToTextResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/SpeechToText",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "SpeechToText"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn create_task(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateTaskRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateTaskResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/CreateTask",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "CreateTask"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_tasks(
            &mut self,
            request: impl tonic::IntoRequest<super::ListTasksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTasksResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/ListTasks",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "ListTasks"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn health(
            &mut self,
            request: impl tonic::IntoRequest<super::HealthRequest>,
        ) -> std::result::Result<tonic::Response<super::HealthResponse>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ModelGateway/Health",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.ModelGateway", "Health"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod model_gateway_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ModelGatewayServer.
    #[async_trait]
    pub trait ModelGateway: std::marker::Send + std::marker::Sync + 'static {
        async fn invoke(
            &self,
            request: tonic::Request<super::InvokeRequest>,
        ) -> std::result::Result<tonic::Response<super::InvokeResponse>, tonic::Status>;
        /// Server streaming response type for the InvokeStream method.
        type InvokeStreamStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::InvokeChunk, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        async fn invoke_stream(
            &self,
            request: tonic::Request<super::InvokeRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::InvokeStreamStream>,
            tonic::Status,
        >;
        async fn fetch(
            &self,
            request: tonic::Request<super::FetchRequest>,
        ) -> std::result::Result<tonic::Response<super::FetchResponse>, tonic::Status>;
        async fn extract_structured(
            &self,
            request: tonic::Request<super::ExtractStructuredRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExtractStructuredResponse>,
            tonic::Status,
        >;
        async fn web_search(
            &self,
            request: tonic::Request<super::WebSearchRequest>,
        ) -> std::result::Result<
            tonic::Response<super::WebSearchResponse>,
            tonic::Status,
        >;
        async fn sleep(
            &self,
            request: tonic::Request<super::SleepRequest>,
        ) -> std::result::Result<tonic::Response<super::SleepResponse>, tonic::Status>;
        async fn remote_trigger(
            &self,
            request: tonic::Request<super::RemoteTriggerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RemoteTriggerResponse>,
            tonic::Status,
        >;
        async fn send_message(
            &self,
            request: tonic::Request<super::SendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SendMessageResponse>,
            tonic::Status,
        >;
        async fn synthetic_output(
            &self,
            request: tonic::Request<super::SyntheticOutputRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SyntheticOutputResponse>,
            tonic::Status,
        >;
        async fn enter_plan_mode(
            &self,
            request: tonic::Request<super::EnterPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::EnterPlanModeResponse>,
            tonic::Status,
        >;
        async fn exit_plan_mode(
            &self,
            request: tonic::Request<super::ExitPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExitPlanModeResponse>,
            tonic::Status,
        >;
        async fn is_plan_mode(
            &self,
            request: tonic::Request<super::IsPlanModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::IsPlanModeResponse>,
            tonic::Status,
        >;
        async fn team_create(
            &self,
            request: tonic::Request<super::TeamCreateRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamCreateResponse>,
            tonic::Status,
        >;
        async fn team_delete(
            &self,
            request: tonic::Request<super::TeamDeleteRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamDeleteResponse>,
            tonic::Status,
        >;
        async fn team_list(
            &self,
            request: tonic::Request<super::TeamListRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TeamListResponse>,
            tonic::Status,
        >;
        async fn lsp_query(
            &self,
            request: tonic::Request<super::LspQueryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::LspQueryResponse>,
            tonic::Status,
        >;
        async fn request_approval(
            &self,
            request: tonic::Request<super::RequestApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RequestApprovalResponse>,
            tonic::Status,
        >;
        async fn approve_approval(
            &self,
            request: tonic::Request<super::ApproveApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ApproveApprovalResponse>,
            tonic::Status,
        >;
        async fn deny_approval(
            &self,
            request: tonic::Request<super::DenyApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DenyApprovalResponse>,
            tonic::Status,
        >;
        async fn list_pending_approvals(
            &self,
            request: tonic::Request<super::ListPendingApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPendingApprovalsResponse>,
            tonic::Status,
        >;
        async fn record_trajectory(
            &self,
            request: tonic::Request<super::RecordTrajectoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordTrajectoryResponse>,
            tonic::Status,
        >;
        async fn list_trajectories(
            &self,
            request: tonic::Request<super::ListTrajectoriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTrajectoriesResponse>,
            tonic::Status,
        >;
        async fn export_trajectories(
            &self,
            request: tonic::Request<super::ExportTrajectoriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExportTrajectoriesResponse>,
            tonic::Status,
        >;
        async fn list_skills(
            &self,
            request: tonic::Request<super::ListSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSkillsResponse>,
            tonic::Status,
        >;
        async fn get_skill(
            &self,
            request: tonic::Request<super::GetSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSkillResponse>,
            tonic::Status,
        >;
        async fn match_skills(
            &self,
            request: tonic::Request<super::MatchSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MatchSkillsResponse>,
            tonic::Status,
        >;
        async fn register_mcp_server(
            &self,
            request: tonic::Request<super::RegisterMcpServerRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterMcpServerResponse>,
            tonic::Status,
        >;
        async fn list_mcp_servers(
            &self,
            request: tonic::Request<super::ListMcpServersRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMcpServersResponse>,
            tonic::Status,
        >;
        async fn proxy_mcp_tool(
            &self,
            request: tonic::Request<super::ProxyMcpToolRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ProxyMcpToolResponse>,
            tonic::Status,
        >;
        async fn list_mcp_tools(
            &self,
            request: tonic::Request<super::ListMcpToolsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMcpToolsResponse>,
            tonic::Status,
        >;
        async fn register_plugin(
            &self,
            request: tonic::Request<super::RegisterPluginRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterPluginResponse>,
            tonic::Status,
        >;
        async fn list_plugins(
            &self,
            request: tonic::Request<super::ListPluginsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPluginsResponse>,
            tonic::Status,
        >;
        async fn set_plugin_enabled(
            &self,
            request: tonic::Request<super::SetPluginEnabledRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPluginEnabledResponse>,
            tonic::Status,
        >;
        async fn list_commands(
            &self,
            request: tonic::Request<super::ListCommandsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListCommandsResponse>,
            tonic::Status,
        >;
        async fn execute_command(
            &self,
            request: tonic::Request<super::ExecuteCommandRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ExecuteCommandResponse>,
            tonic::Status,
        >;
        async fn register_hook(
            &self,
            request: tonic::Request<super::RegisterHookRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RegisterHookResponse>,
            tonic::Status,
        >;
        async fn list_hooks(
            &self,
            request: tonic::Request<super::ListHooksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListHooksResponse>,
            tonic::Status,
        >;
        async fn check_permission(
            &self,
            request: tonic::Request<super::CheckPermissionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CheckPermissionResponse>,
            tonic::Status,
        >;
        async fn set_permission(
            &self,
            request: tonic::Request<super::SetPermissionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPermissionResponse>,
            tonic::Status,
        >;
        async fn get_policy(
            &self,
            request: tonic::Request<super::GetPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPolicyResponse>,
            tonic::Status,
        >;
        async fn set_policy(
            &self,
            request: tonic::Request<super::SetPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetPolicyResponse>,
            tonic::Status,
        >;
        async fn append_thread_message(
            &self,
            request: tonic::Request<super::AppendThreadMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendThreadMessageResponse>,
            tonic::Status,
        >;
        async fn list_thread_messages(
            &self,
            request: tonic::Request<super::ListThreadMessagesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadMessagesResponse>,
            tonic::Status,
        >;
        async fn get_analytics(
            &self,
            request: tonic::Request<super::GetAnalyticsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetAnalyticsResponse>,
            tonic::Status,
        >;
        async fn text_to_speech(
            &self,
            request: tonic::Request<super::TextToSpeechRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TextToSpeechResponse>,
            tonic::Status,
        >;
        async fn speech_to_text(
            &self,
            request: tonic::Request<super::SpeechToTextRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SpeechToTextResponse>,
            tonic::Status,
        >;
        async fn create_task(
            &self,
            request: tonic::Request<super::CreateTaskRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateTaskResponse>,
            tonic::Status,
        >;
        async fn list_tasks(
            &self,
            request: tonic::Request<super::ListTasksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTasksResponse>,
            tonic::Status,
        >;
        async fn health(
            &self,
            request: tonic::Request<super::HealthRequest>,
        ) -> std::result::Result<tonic::Response<super::HealthResponse>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct ModelGatewayServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> ModelGatewayServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ModelGatewayServer<T>
    where
        T: ModelGateway,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.ModelGateway/Invoke" => {
                    #[allow(non_camel_case_types)]
                    struct InvokeSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::InvokeRequest>
                    for InvokeSvc<T> {
                        type Response = super::InvokeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InvokeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::invoke(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = InvokeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/InvokeStream" => {
                    #[allow(non_camel_case_types)]
                    struct InvokeStreamSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::ServerStreamingService<super::InvokeRequest>
                    for InvokeStreamSvc<T> {
                        type Response = super::InvokeChunk;
                        type ResponseStream = T::InvokeStreamStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::InvokeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::invoke_stream(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = InvokeStreamSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/Fetch" => {
                    #[allow(non_camel_case_types)]
                    struct FetchSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::FetchRequest> for FetchSvc<T> {
                        type Response = super::FetchResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::FetchRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::fetch(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = FetchSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ExtractStructured" => {
                    #[allow(non_camel_case_types)]
                    struct ExtractStructuredSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ExtractStructuredRequest>
                    for ExtractStructuredSvc<T> {
                        type Response = super::ExtractStructuredResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExtractStructuredRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::extract_structured(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExtractStructuredSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/WebSearch" => {
                    #[allow(non_camel_case_types)]
                    struct WebSearchSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::WebSearchRequest>
                    for WebSearchSvc<T> {
                        type Response = super::WebSearchResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::WebSearchRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::web_search(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = WebSearchSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/Sleep" => {
                    #[allow(non_camel_case_types)]
                    struct SleepSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SleepRequest> for SleepSvc<T> {
                        type Response = super::SleepResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SleepRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::sleep(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SleepSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RemoteTrigger" => {
                    #[allow(non_camel_case_types)]
                    struct RemoteTriggerSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RemoteTriggerRequest>
                    for RemoteTriggerSvc<T> {
                        type Response = super::RemoteTriggerResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RemoteTriggerRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::remote_trigger(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RemoteTriggerSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SendMessage" => {
                    #[allow(non_camel_case_types)]
                    struct SendMessageSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SendMessageRequest>
                    for SendMessageSvc<T> {
                        type Response = super::SendMessageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SendMessageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::send_message(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SendMessageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SyntheticOutput" => {
                    #[allow(non_camel_case_types)]
                    struct SyntheticOutputSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SyntheticOutputRequest>
                    for SyntheticOutputSvc<T> {
                        type Response = super::SyntheticOutputResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SyntheticOutputRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::synthetic_output(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SyntheticOutputSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/EnterPlanMode" => {
                    #[allow(non_camel_case_types)]
                    struct EnterPlanModeSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::EnterPlanModeRequest>
                    for EnterPlanModeSvc<T> {
                        type Response = super::EnterPlanModeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::EnterPlanModeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::enter_plan_mode(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = EnterPlanModeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ExitPlanMode" => {
                    #[allow(non_camel_case_types)]
                    struct ExitPlanModeSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ExitPlanModeRequest>
                    for ExitPlanModeSvc<T> {
                        type Response = super::ExitPlanModeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExitPlanModeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::exit_plan_mode(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExitPlanModeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/IsPlanMode" => {
                    #[allow(non_camel_case_types)]
                    struct IsPlanModeSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::IsPlanModeRequest>
                    for IsPlanModeSvc<T> {
                        type Response = super::IsPlanModeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::IsPlanModeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::is_plan_mode(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = IsPlanModeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/TeamCreate" => {
                    #[allow(non_camel_case_types)]
                    struct TeamCreateSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::TeamCreateRequest>
                    for TeamCreateSvc<T> {
                        type Response = super::TeamCreateResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TeamCreateRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::team_create(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TeamCreateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/TeamDelete" => {
                    #[allow(non_camel_case_types)]
                    struct TeamDeleteSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::TeamDeleteRequest>
                    for TeamDeleteSvc<T> {
                        type Response = super::TeamDeleteResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TeamDeleteRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::team_delete(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TeamDeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/TeamList" => {
                    #[allow(non_camel_case_types)]
                    struct TeamListSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::TeamListRequest>
                    for TeamListSvc<T> {
                        type Response = super::TeamListResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TeamListRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::team_list(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TeamListSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/LspQuery" => {
                    #[allow(non_camel_case_types)]
                    struct LspQuerySvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::LspQueryRequest>
                    for LspQuerySvc<T> {
                        type Response = super::LspQueryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::LspQueryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::lsp_query(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = LspQuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RequestApproval" => {
                    #[allow(non_camel_case_types)]
                    struct RequestApprovalSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RequestApprovalRequest>
                    for RequestApprovalSvc<T> {
                        type Response = super::RequestApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RequestApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::request_approval(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RequestApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ApproveApproval" => {
                    #[allow(non_camel_case_types)]
                    struct ApproveApprovalSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ApproveApprovalRequest>
                    for ApproveApprovalSvc<T> {
                        type Response = super::ApproveApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ApproveApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::approve_approval(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ApproveApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/DenyApproval" => {
                    #[allow(non_camel_case_types)]
                    struct DenyApprovalSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::DenyApprovalRequest>
                    for DenyApprovalSvc<T> {
                        type Response = super::DenyApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DenyApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::deny_approval(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DenyApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListPendingApprovals" => {
                    #[allow(non_camel_case_types)]
                    struct ListPendingApprovalsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListPendingApprovalsRequest>
                    for ListPendingApprovalsSvc<T> {
                        type Response = super::ListPendingApprovalsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListPendingApprovalsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_pending_approvals(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListPendingApprovalsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RecordTrajectory" => {
                    #[allow(non_camel_case_types)]
                    struct RecordTrajectorySvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RecordTrajectoryRequest>
                    for RecordTrajectorySvc<T> {
                        type Response = super::RecordTrajectoryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordTrajectoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::record_trajectory(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RecordTrajectorySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListTrajectories" => {
                    #[allow(non_camel_case_types)]
                    struct ListTrajectoriesSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListTrajectoriesRequest>
                    for ListTrajectoriesSvc<T> {
                        type Response = super::ListTrajectoriesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListTrajectoriesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_trajectories(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListTrajectoriesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ExportTrajectories" => {
                    #[allow(non_camel_case_types)]
                    struct ExportTrajectoriesSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ExportTrajectoriesRequest>
                    for ExportTrajectoriesSvc<T> {
                        type Response = super::ExportTrajectoriesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExportTrajectoriesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::export_trajectories(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExportTrajectoriesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListSkills" => {
                    #[allow(non_camel_case_types)]
                    struct ListSkillsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListSkillsRequest>
                    for ListSkillsSvc<T> {
                        type Response = super::ListSkillsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListSkillsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_skills(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListSkillsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/GetSkill" => {
                    #[allow(non_camel_case_types)]
                    struct GetSkillSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::GetSkillRequest>
                    for GetSkillSvc<T> {
                        type Response = super::GetSkillResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSkillRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::get_skill(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetSkillSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/MatchSkills" => {
                    #[allow(non_camel_case_types)]
                    struct MatchSkillsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::MatchSkillsRequest>
                    for MatchSkillsSvc<T> {
                        type Response = super::MatchSkillsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MatchSkillsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::match_skills(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = MatchSkillsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RegisterMcpServer" => {
                    #[allow(non_camel_case_types)]
                    struct RegisterMcpServerSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RegisterMcpServerRequest>
                    for RegisterMcpServerSvc<T> {
                        type Response = super::RegisterMcpServerResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RegisterMcpServerRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::register_mcp_server(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RegisterMcpServerSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListMcpServers" => {
                    #[allow(non_camel_case_types)]
                    struct ListMcpServersSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListMcpServersRequest>
                    for ListMcpServersSvc<T> {
                        type Response = super::ListMcpServersResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListMcpServersRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_mcp_servers(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListMcpServersSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ProxyMcpTool" => {
                    #[allow(non_camel_case_types)]
                    struct ProxyMcpToolSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ProxyMcpToolRequest>
                    for ProxyMcpToolSvc<T> {
                        type Response = super::ProxyMcpToolResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ProxyMcpToolRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::proxy_mcp_tool(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ProxyMcpToolSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListMcpTools" => {
                    #[allow(non_camel_case_types)]
                    struct ListMcpToolsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListMcpToolsRequest>
                    for ListMcpToolsSvc<T> {
                        type Response = super::ListMcpToolsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListMcpToolsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_mcp_tools(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListMcpToolsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RegisterPlugin" => {
                    #[allow(non_camel_case_types)]
                    struct RegisterPluginSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RegisterPluginRequest>
                    for RegisterPluginSvc<T> {
                        type Response = super::RegisterPluginResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RegisterPluginRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::register_plugin(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RegisterPluginSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListPlugins" => {
                    #[allow(non_camel_case_types)]
                    struct ListPluginsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListPluginsRequest>
                    for ListPluginsSvc<T> {
                        type Response = super::ListPluginsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListPluginsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_plugins(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListPluginsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SetPluginEnabled" => {
                    #[allow(non_camel_case_types)]
                    struct SetPluginEnabledSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SetPluginEnabledRequest>
                    for SetPluginEnabledSvc<T> {
                        type Response = super::SetPluginEnabledResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetPluginEnabledRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::set_plugin_enabled(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetPluginEnabledSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListCommands" => {
                    #[allow(non_camel_case_types)]
                    struct ListCommandsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListCommandsRequest>
                    for ListCommandsSvc<T> {
                        type Response = super::ListCommandsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListCommandsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_commands(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListCommandsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ExecuteCommand" => {
                    #[allow(non_camel_case_types)]
                    struct ExecuteCommandSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ExecuteCommandRequest>
                    for ExecuteCommandSvc<T> {
                        type Response = super::ExecuteCommandResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExecuteCommandRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::execute_command(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ExecuteCommandSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/RegisterHook" => {
                    #[allow(non_camel_case_types)]
                    struct RegisterHookSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::RegisterHookRequest>
                    for RegisterHookSvc<T> {
                        type Response = super::RegisterHookResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RegisterHookRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::register_hook(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RegisterHookSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListHooks" => {
                    #[allow(non_camel_case_types)]
                    struct ListHooksSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListHooksRequest>
                    for ListHooksSvc<T> {
                        type Response = super::ListHooksResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListHooksRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_hooks(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListHooksSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/CheckPermission" => {
                    #[allow(non_camel_case_types)]
                    struct CheckPermissionSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::CheckPermissionRequest>
                    for CheckPermissionSvc<T> {
                        type Response = super::CheckPermissionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CheckPermissionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::check_permission(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CheckPermissionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SetPermission" => {
                    #[allow(non_camel_case_types)]
                    struct SetPermissionSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SetPermissionRequest>
                    for SetPermissionSvc<T> {
                        type Response = super::SetPermissionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetPermissionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::set_permission(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetPermissionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/GetPolicy" => {
                    #[allow(non_camel_case_types)]
                    struct GetPolicySvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::GetPolicyRequest>
                    for GetPolicySvc<T> {
                        type Response = super::GetPolicyResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetPolicyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::get_policy(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetPolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SetPolicy" => {
                    #[allow(non_camel_case_types)]
                    struct SetPolicySvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SetPolicyRequest>
                    for SetPolicySvc<T> {
                        type Response = super::SetPolicyResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetPolicyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::set_policy(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetPolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/AppendThreadMessage" => {
                    #[allow(non_camel_case_types)]
                    struct AppendThreadMessageSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::AppendThreadMessageRequest>
                    for AppendThreadMessageSvc<T> {
                        type Response = super::AppendThreadMessageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendThreadMessageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::append_thread_message(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AppendThreadMessageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListThreadMessages" => {
                    #[allow(non_camel_case_types)]
                    struct ListThreadMessagesSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListThreadMessagesRequest>
                    for ListThreadMessagesSvc<T> {
                        type Response = super::ListThreadMessagesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListThreadMessagesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_thread_messages(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListThreadMessagesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/GetAnalytics" => {
                    #[allow(non_camel_case_types)]
                    struct GetAnalyticsSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::GetAnalyticsRequest>
                    for GetAnalyticsSvc<T> {
                        type Response = super::GetAnalyticsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetAnalyticsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::get_analytics(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetAnalyticsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/TextToSpeech" => {
                    #[allow(non_camel_case_types)]
                    struct TextToSpeechSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::TextToSpeechRequest>
                    for TextToSpeechSvc<T> {
                        type Response = super::TextToSpeechResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TextToSpeechRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::text_to_speech(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TextToSpeechSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/SpeechToText" => {
                    #[allow(non_camel_case_types)]
                    struct SpeechToTextSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::SpeechToTextRequest>
                    for SpeechToTextSvc<T> {
                        type Response = super::SpeechToTextResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SpeechToTextRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::speech_to_text(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SpeechToTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/CreateTask" => {
                    #[allow(non_camel_case_types)]
                    struct CreateTaskSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::CreateTaskRequest>
                    for CreateTaskSvc<T> {
                        type Response = super::CreateTaskResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateTaskRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::create_task(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateTaskSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/ListTasks" => {
                    #[allow(non_camel_case_types)]
                    struct ListTasksSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::ListTasksRequest>
                    for ListTasksSvc<T> {
                        type Response = super::ListTasksResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListTasksRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::list_tasks(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListTasksSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ModelGateway/Health" => {
                    #[allow(non_camel_case_types)]
                    struct HealthSvc<T: ModelGateway>(pub Arc<T>);
                    impl<
                        T: ModelGateway,
                    > tonic::server::UnaryService<super::HealthRequest>
                    for HealthSvc<T> {
                        type Response = super::HealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::HealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ModelGateway>::health(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = HealthSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for ModelGatewayServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.ModelGateway";
    impl<T> tonic::server::NamedService for ModelGatewayServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod memory_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct MemoryServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl MemoryServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> MemoryServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> MemoryServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            MemoryServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn search_memory(
            &mut self,
            request: impl tonic::IntoRequest<super::SearchMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SearchMemoryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.MemoryService/SearchMemory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.MemoryService", "SearchMemory"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn index_memory(
            &mut self,
            request: impl tonic::IntoRequest<super::IndexMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::IndexMemoryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.MemoryService/IndexMemory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.MemoryService", "IndexMemory"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_memory(
            &mut self,
            request: impl tonic::IntoRequest<super::ListMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMemoryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.MemoryService/ListMemory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.MemoryService", "ListMemory"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn delete_memory(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteMemoryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.MemoryService/DeleteMemory",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.MemoryService", "DeleteMemory"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn health(
            &mut self,
            request: impl tonic::IntoRequest<super::MemoryHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MemoryHealthResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.MemoryService/Health",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.MemoryService", "Health"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod memory_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with MemoryServiceServer.
    #[async_trait]
    pub trait MemoryService: std::marker::Send + std::marker::Sync + 'static {
        async fn search_memory(
            &self,
            request: tonic::Request<super::SearchMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SearchMemoryResponse>,
            tonic::Status,
        >;
        async fn index_memory(
            &self,
            request: tonic::Request<super::IndexMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::IndexMemoryResponse>,
            tonic::Status,
        >;
        async fn list_memory(
            &self,
            request: tonic::Request<super::ListMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMemoryResponse>,
            tonic::Status,
        >;
        async fn delete_memory(
            &self,
            request: tonic::Request<super::DeleteMemoryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DeleteMemoryResponse>,
            tonic::Status,
        >;
        async fn health(
            &self,
            request: tonic::Request<super::MemoryHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::MemoryHealthResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct MemoryServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> MemoryServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for MemoryServiceServer<T>
    where
        T: MemoryService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.MemoryService/SearchMemory" => {
                    #[allow(non_camel_case_types)]
                    struct SearchMemorySvc<T: MemoryService>(pub Arc<T>);
                    impl<
                        T: MemoryService,
                    > tonic::server::UnaryService<super::SearchMemoryRequest>
                    for SearchMemorySvc<T> {
                        type Response = super::SearchMemoryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SearchMemoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as MemoryService>::search_memory(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SearchMemorySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.MemoryService/IndexMemory" => {
                    #[allow(non_camel_case_types)]
                    struct IndexMemorySvc<T: MemoryService>(pub Arc<T>);
                    impl<
                        T: MemoryService,
                    > tonic::server::UnaryService<super::IndexMemoryRequest>
                    for IndexMemorySvc<T> {
                        type Response = super::IndexMemoryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::IndexMemoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as MemoryService>::index_memory(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = IndexMemorySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.MemoryService/ListMemory" => {
                    #[allow(non_camel_case_types)]
                    struct ListMemorySvc<T: MemoryService>(pub Arc<T>);
                    impl<
                        T: MemoryService,
                    > tonic::server::UnaryService<super::ListMemoryRequest>
                    for ListMemorySvc<T> {
                        type Response = super::ListMemoryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListMemoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as MemoryService>::list_memory(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListMemorySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.MemoryService/DeleteMemory" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteMemorySvc<T: MemoryService>(pub Arc<T>);
                    impl<
                        T: MemoryService,
                    > tonic::server::UnaryService<super::DeleteMemoryRequest>
                    for DeleteMemorySvc<T> {
                        type Response = super::DeleteMemoryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteMemoryRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as MemoryService>::delete_memory(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DeleteMemorySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.MemoryService/Health" => {
                    #[allow(non_camel_case_types)]
                    struct HealthSvc<T: MemoryService>(pub Arc<T>);
                    impl<
                        T: MemoryService,
                    > tonic::server::UnaryService<super::MemoryHealthRequest>
                    for HealthSvc<T> {
                        type Response = super::MemoryHealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::MemoryHealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as MemoryService>::health(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = HealthSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for MemoryServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.MemoryService";
    impl<T> tonic::server::NamedService for MemoryServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod orchestration_core_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct OrchestrationCoreServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl OrchestrationCoreServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> OrchestrationCoreServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> OrchestrationCoreServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            OrchestrationCoreServiceClient::new(
                InterceptedService::new(inner, interceptor),
            )
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn list_plans(
            &mut self,
            request: impl tonic::IntoRequest<super::ListPlansRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPlansResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/ListPlans",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "ListPlans",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_plan(
            &mut self,
            request: impl tonic::IntoRequest<super::GetPlanRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPlanResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/GetPlan",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.OrchestrationCoreService", "GetPlan"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn transition_plan(
            &mut self,
            request: impl tonic::IntoRequest<super::TransitionPlanRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TransitionPlanResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/TransitionPlan",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "TransitionPlan",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_todos(
            &mut self,
            request: impl tonic::IntoRequest<super::ListTodosRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTodosResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/ListTodos",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "ListTodos",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_todo(
            &mut self,
            request: impl tonic::IntoRequest<super::GetTodoRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetTodoResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/GetTodo",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.OrchestrationCoreService", "GetTodo"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn transition_todo(
            &mut self,
            request: impl tonic::IntoRequest<super::TransitionTodoRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TransitionTodoResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/TransitionTodo",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "TransitionTodo",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn create_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/CreateApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "CreateApproval",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_approvals(
            &mut self,
            request: impl tonic::IntoRequest<super::ListApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListApprovalsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/ListApprovals",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "ListApprovals",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_pending_approvals(
            &mut self,
            request: impl tonic::IntoRequest<super::OrgPendingApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::OrgPendingApprovalsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/ListPendingApprovals",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "ListPendingApprovals",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::GetApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/GetApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "GetApproval",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn decide_approval(
            &mut self,
            request: impl tonic::IntoRequest<super::DecideApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DecideApprovalResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/DecideApproval",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "DecideApproval",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn claim_approval_deliveries(
            &mut self,
            request: impl tonic::IntoRequest<super::ClaimApprovalDeliveriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ClaimApprovalDeliveriesResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/ClaimApprovalDeliveries",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "ClaimApprovalDeliveries",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_approval_continuation(
            &mut self,
            request: impl tonic::IntoRequest<super::GetApprovalContinuationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetApprovalContinuationResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/GetApprovalContinuation",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "GetApprovalContinuation",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn record_approval_continuation_started(
            &mut self,
            request: impl tonic::IntoRequest<
                super::RecordApprovalContinuationStartedRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<super::RecordApprovalContinuationStartedResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/RecordApprovalContinuationStarted",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "RecordApprovalContinuationStarted",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn record_approval_continuation_outcome(
            &mut self,
            request: impl tonic::IntoRequest<
                super::RecordApprovalContinuationOutcomeRequest,
            >,
        ) -> std::result::Result<
            tonic::Response<super::RecordApprovalContinuationOutcomeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/RecordApprovalContinuationOutcome",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "RecordApprovalContinuationOutcome",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn acknowledge_approval_delivery(
            &mut self,
            request: impl tonic::IntoRequest<super::AcknowledgeApprovalDeliveryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcknowledgeApprovalDeliveryResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/AcknowledgeApprovalDelivery",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "AcknowledgeApprovalDelivery",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn get_subagent_lineage(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSubagentLineageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSubagentLineageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/GetSubagentLineage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "GetSubagentLineage",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn attach_subagent(
            &mut self,
            request: impl tonic::IntoRequest<super::AttachSubagentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AttachSubagentResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/AttachSubagent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "AttachSubagent",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn stream_run_events(
            &mut self,
            request: impl tonic::IntoRequest<super::StreamRunEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::OrchestrationEvent>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/StreamRunEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "StreamRunEvents",
                    ),
                );
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn record_orchestration_event(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordOrchestrationEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordOrchestrationEventResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestrationCoreService/RecordOrchestrationEvent",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestrationCoreService",
                        "RecordOrchestrationEvent",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod orchestration_core_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with OrchestrationCoreServiceServer.
    #[async_trait]
    pub trait OrchestrationCoreService: std::marker::Send + std::marker::Sync + 'static {
        async fn list_plans(
            &self,
            request: tonic::Request<super::ListPlansRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPlansResponse>,
            tonic::Status,
        >;
        async fn get_plan(
            &self,
            request: tonic::Request<super::GetPlanRequest>,
        ) -> std::result::Result<tonic::Response<super::GetPlanResponse>, tonic::Status>;
        async fn transition_plan(
            &self,
            request: tonic::Request<super::TransitionPlanRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TransitionPlanResponse>,
            tonic::Status,
        >;
        async fn list_todos(
            &self,
            request: tonic::Request<super::ListTodosRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListTodosResponse>,
            tonic::Status,
        >;
        async fn get_todo(
            &self,
            request: tonic::Request<super::GetTodoRequest>,
        ) -> std::result::Result<tonic::Response<super::GetTodoResponse>, tonic::Status>;
        async fn transition_todo(
            &self,
            request: tonic::Request<super::TransitionTodoRequest>,
        ) -> std::result::Result<
            tonic::Response<super::TransitionTodoResponse>,
            tonic::Status,
        >;
        async fn create_approval(
            &self,
            request: tonic::Request<super::CreateApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateApprovalResponse>,
            tonic::Status,
        >;
        async fn list_approvals(
            &self,
            request: tonic::Request<super::ListApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListApprovalsResponse>,
            tonic::Status,
        >;
        async fn list_pending_approvals(
            &self,
            request: tonic::Request<super::OrgPendingApprovalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::OrgPendingApprovalsResponse>,
            tonic::Status,
        >;
        async fn get_approval(
            &self,
            request: tonic::Request<super::GetApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetApprovalResponse>,
            tonic::Status,
        >;
        async fn decide_approval(
            &self,
            request: tonic::Request<super::DecideApprovalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::DecideApprovalResponse>,
            tonic::Status,
        >;
        async fn claim_approval_deliveries(
            &self,
            request: tonic::Request<super::ClaimApprovalDeliveriesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ClaimApprovalDeliveriesResponse>,
            tonic::Status,
        >;
        async fn get_approval_continuation(
            &self,
            request: tonic::Request<super::GetApprovalContinuationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetApprovalContinuationResponse>,
            tonic::Status,
        >;
        async fn record_approval_continuation_started(
            &self,
            request: tonic::Request<super::RecordApprovalContinuationStartedRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordApprovalContinuationStartedResponse>,
            tonic::Status,
        >;
        async fn record_approval_continuation_outcome(
            &self,
            request: tonic::Request<super::RecordApprovalContinuationOutcomeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordApprovalContinuationOutcomeResponse>,
            tonic::Status,
        >;
        async fn acknowledge_approval_delivery(
            &self,
            request: tonic::Request<super::AcknowledgeApprovalDeliveryRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcknowledgeApprovalDeliveryResponse>,
            tonic::Status,
        >;
        async fn get_subagent_lineage(
            &self,
            request: tonic::Request<super::GetSubagentLineageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetSubagentLineageResponse>,
            tonic::Status,
        >;
        async fn attach_subagent(
            &self,
            request: tonic::Request<super::AttachSubagentRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AttachSubagentResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the StreamRunEvents method.
        type StreamRunEventsStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::OrchestrationEvent, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        async fn stream_run_events(
            &self,
            request: tonic::Request<super::StreamRunEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::StreamRunEventsStream>,
            tonic::Status,
        >;
        async fn record_orchestration_event(
            &self,
            request: tonic::Request<super::RecordOrchestrationEventRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordOrchestrationEventResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct OrchestrationCoreServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> OrchestrationCoreServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>>
    for OrchestrationCoreServiceServer<T>
    where
        T: OrchestrationCoreService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.OrchestrationCoreService/ListPlans" => {
                    #[allow(non_camel_case_types)]
                    struct ListPlansSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::ListPlansRequest>
                    for ListPlansSvc<T> {
                        type Response = super::ListPlansResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListPlansRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::list_plans(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListPlansSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/GetPlan" => {
                    #[allow(non_camel_case_types)]
                    struct GetPlanSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::GetPlanRequest>
                    for GetPlanSvc<T> {
                        type Response = super::GetPlanResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetPlanRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::get_plan(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetPlanSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/TransitionPlan" => {
                    #[allow(non_camel_case_types)]
                    struct TransitionPlanSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::TransitionPlanRequest>
                    for TransitionPlanSvc<T> {
                        type Response = super::TransitionPlanResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TransitionPlanRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::transition_plan(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TransitionPlanSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/ListTodos" => {
                    #[allow(non_camel_case_types)]
                    struct ListTodosSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::ListTodosRequest>
                    for ListTodosSvc<T> {
                        type Response = super::ListTodosResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListTodosRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::list_todos(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListTodosSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/GetTodo" => {
                    #[allow(non_camel_case_types)]
                    struct GetTodoSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::GetTodoRequest>
                    for GetTodoSvc<T> {
                        type Response = super::GetTodoResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetTodoRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::get_todo(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetTodoSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/TransitionTodo" => {
                    #[allow(non_camel_case_types)]
                    struct TransitionTodoSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::TransitionTodoRequest>
                    for TransitionTodoSvc<T> {
                        type Response = super::TransitionTodoResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::TransitionTodoRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::transition_todo(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = TransitionTodoSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/CreateApproval" => {
                    #[allow(non_camel_case_types)]
                    struct CreateApprovalSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::CreateApprovalRequest>
                    for CreateApprovalSvc<T> {
                        type Response = super::CreateApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::create_approval(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/ListApprovals" => {
                    #[allow(non_camel_case_types)]
                    struct ListApprovalsSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::ListApprovalsRequest>
                    for ListApprovalsSvc<T> {
                        type Response = super::ListApprovalsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListApprovalsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::list_approvals(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListApprovalsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/ListPendingApprovals" => {
                    #[allow(non_camel_case_types)]
                    struct ListPendingApprovalsSvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::OrgPendingApprovalsRequest>
                    for ListPendingApprovalsSvc<T> {
                        type Response = super::OrgPendingApprovalsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::OrgPendingApprovalsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::list_pending_approvals(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListPendingApprovalsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/GetApproval" => {
                    #[allow(non_camel_case_types)]
                    struct GetApprovalSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::GetApprovalRequest>
                    for GetApprovalSvc<T> {
                        type Response = super::GetApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::get_approval(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/DecideApproval" => {
                    #[allow(non_camel_case_types)]
                    struct DecideApprovalSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::DecideApprovalRequest>
                    for DecideApprovalSvc<T> {
                        type Response = super::DecideApprovalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DecideApprovalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::decide_approval(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DecideApprovalSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/ClaimApprovalDeliveries" => {
                    #[allow(non_camel_case_types)]
                    struct ClaimApprovalDeliveriesSvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::ClaimApprovalDeliveriesRequest>
                    for ClaimApprovalDeliveriesSvc<T> {
                        type Response = super::ClaimApprovalDeliveriesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::ClaimApprovalDeliveriesRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::claim_approval_deliveries(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ClaimApprovalDeliveriesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/GetApprovalContinuation" => {
                    #[allow(non_camel_case_types)]
                    struct GetApprovalContinuationSvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::GetApprovalContinuationRequest>
                    for GetApprovalContinuationSvc<T> {
                        type Response = super::GetApprovalContinuationResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::GetApprovalContinuationRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::get_approval_continuation(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetApprovalContinuationSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/RecordApprovalContinuationStarted" => {
                    #[allow(non_camel_case_types)]
                    struct RecordApprovalContinuationStartedSvc<
                        T: OrchestrationCoreService,
                    >(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<
                        super::RecordApprovalContinuationStartedRequest,
                    > for RecordApprovalContinuationStartedSvc<T> {
                        type Response = super::RecordApprovalContinuationStartedResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::RecordApprovalContinuationStartedRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::record_approval_continuation_started(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RecordApprovalContinuationStartedSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/RecordApprovalContinuationOutcome" => {
                    #[allow(non_camel_case_types)]
                    struct RecordApprovalContinuationOutcomeSvc<
                        T: OrchestrationCoreService,
                    >(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<
                        super::RecordApprovalContinuationOutcomeRequest,
                    > for RecordApprovalContinuationOutcomeSvc<T> {
                        type Response = super::RecordApprovalContinuationOutcomeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::RecordApprovalContinuationOutcomeRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::record_approval_continuation_outcome(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RecordApprovalContinuationOutcomeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/AcknowledgeApprovalDelivery" => {
                    #[allow(non_camel_case_types)]
                    struct AcknowledgeApprovalDeliverySvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<
                        super::AcknowledgeApprovalDeliveryRequest,
                    > for AcknowledgeApprovalDeliverySvc<T> {
                        type Response = super::AcknowledgeApprovalDeliveryResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::AcknowledgeApprovalDeliveryRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::acknowledge_approval_delivery(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AcknowledgeApprovalDeliverySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/GetSubagentLineage" => {
                    #[allow(non_camel_case_types)]
                    struct GetSubagentLineageSvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::GetSubagentLineageRequest>
                    for GetSubagentLineageSvc<T> {
                        type Response = super::GetSubagentLineageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSubagentLineageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::get_subagent_lineage(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetSubagentLineageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/AttachSubagent" => {
                    #[allow(non_camel_case_types)]
                    struct AttachSubagentSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::AttachSubagentRequest>
                    for AttachSubagentSvc<T> {
                        type Response = super::AttachSubagentResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AttachSubagentRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::attach_subagent(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AttachSubagentSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/StreamRunEvents" => {
                    #[allow(non_camel_case_types)]
                    struct StreamRunEventsSvc<T: OrchestrationCoreService>(pub Arc<T>);
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::ServerStreamingService<
                        super::StreamRunEventsRequest,
                    > for StreamRunEventsSvc<T> {
                        type Response = super::OrchestrationEvent;
                        type ResponseStream = T::StreamRunEventsStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StreamRunEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::stream_run_events(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StreamRunEventsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.OrchestrationCoreService/RecordOrchestrationEvent" => {
                    #[allow(non_camel_case_types)]
                    struct RecordOrchestrationEventSvc<T: OrchestrationCoreService>(
                        pub Arc<T>,
                    );
                    impl<
                        T: OrchestrationCoreService,
                    > tonic::server::UnaryService<super::RecordOrchestrationEventRequest>
                    for RecordOrchestrationEventSvc<T> {
                        type Response = super::RecordOrchestrationEventResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<
                                super::RecordOrchestrationEventRequest,
                            >,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestrationCoreService>::record_orchestration_event(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RecordOrchestrationEventSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for OrchestrationCoreServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.OrchestrationCoreService";
    impl<T> tonic::server::NamedService for OrchestrationCoreServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod orchestrator_workflow_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct OrchestratorWorkflowServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl OrchestratorWorkflowServiceClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> OrchestratorWorkflowServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> OrchestratorWorkflowServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            OrchestratorWorkflowServiceClient::new(
                InterceptedService::new(inner, interceptor),
            )
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn start_workflow(
            &mut self,
            request: impl tonic::IntoRequest<super::StartWorkflowRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartWorkflowResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.OrchestratorWorkflowService/StartWorkflow",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.OrchestratorWorkflowService",
                        "StartWorkflow",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod orchestrator_workflow_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with OrchestratorWorkflowServiceServer.
    #[async_trait]
    pub trait OrchestratorWorkflowService: std::marker::Send + std::marker::Sync + 'static {
        async fn start_workflow(
            &self,
            request: tonic::Request<super::StartWorkflowRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartWorkflowResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct OrchestratorWorkflowServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> OrchestratorWorkflowServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>>
    for OrchestratorWorkflowServiceServer<T>
    where
        T: OrchestratorWorkflowService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.OrchestratorWorkflowService/StartWorkflow" => {
                    #[allow(non_camel_case_types)]
                    struct StartWorkflowSvc<T: OrchestratorWorkflowService>(pub Arc<T>);
                    impl<
                        T: OrchestratorWorkflowService,
                    > tonic::server::UnaryService<super::StartWorkflowRequest>
                    for StartWorkflowSvc<T> {
                        type Response = super::StartWorkflowResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StartWorkflowRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as OrchestratorWorkflowService>::start_workflow(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StartWorkflowSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for OrchestratorWorkflowServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.OrchestratorWorkflowService";
    impl<T> tonic::server::NamedService for OrchestratorWorkflowServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod routing_policy_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct RoutingPolicyClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl RoutingPolicyClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> RoutingPolicyClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> RoutingPolicyClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            RoutingPolicyClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn get_policy(
            &mut self,
            request: impl tonic::IntoRequest<super::GetRoutingPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RoutingPolicyMessage>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RoutingPolicy/GetPolicy",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RoutingPolicy", "GetPolicy"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_policy(
            &mut self,
            request: impl tonic::IntoRequest<super::SetRoutingPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RoutingPolicyMessage>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.RoutingPolicy/SetPolicy",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.RoutingPolicy", "SetPolicy"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod routing_policy_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with RoutingPolicyServer.
    #[async_trait]
    pub trait RoutingPolicy: std::marker::Send + std::marker::Sync + 'static {
        async fn get_policy(
            &self,
            request: tonic::Request<super::GetRoutingPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RoutingPolicyMessage>,
            tonic::Status,
        >;
        async fn set_policy(
            &self,
            request: tonic::Request<super::SetRoutingPolicyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RoutingPolicyMessage>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct RoutingPolicyServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> RoutingPolicyServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for RoutingPolicyServer<T>
    where
        T: RoutingPolicy,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.RoutingPolicy/GetPolicy" => {
                    #[allow(non_camel_case_types)]
                    struct GetPolicySvc<T: RoutingPolicy>(pub Arc<T>);
                    impl<
                        T: RoutingPolicy,
                    > tonic::server::UnaryService<super::GetRoutingPolicyRequest>
                    for GetPolicySvc<T> {
                        type Response = super::RoutingPolicyMessage;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetRoutingPolicyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RoutingPolicy>::get_policy(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetPolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.RoutingPolicy/SetPolicy" => {
                    #[allow(non_camel_case_types)]
                    struct SetPolicySvc<T: RoutingPolicy>(pub Arc<T>);
                    impl<
                        T: RoutingPolicy,
                    > tonic::server::UnaryService<super::SetRoutingPolicyRequest>
                    for SetPolicySvc<T> {
                        type Response = super::RoutingPolicyMessage;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetRoutingPolicyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as RoutingPolicy>::set_policy(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetPolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for RoutingPolicyServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.RoutingPolicy";
    impl<T> tonic::server::NamedService for RoutingPolicyServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod sandbox_manager_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct SandboxManagerClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl SandboxManagerClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> SandboxManagerClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> SandboxManagerClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            SandboxManagerClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn acquire_lease(
            &mut self,
            request: impl tonic::IntoRequest<super::AcquireLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcquireLeaseResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SandboxManager/AcquireLease",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SandboxManager", "AcquireLease"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn release_lease(
            &mut self,
            request: impl tonic::IntoRequest<super::ReleaseLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReleaseLeaseResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SandboxManager/ReleaseLease",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SandboxManager", "ReleaseLease"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn snapshot_sandbox(
            &mut self,
            request: impl tonic::IntoRequest<super::SnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SnapshotResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SandboxManager/SnapshotSandbox",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SandboxManager", "SnapshotSandbox"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn health(
            &mut self,
            request: impl tonic::IntoRequest<super::SandboxHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SandboxHealthResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SandboxManager/Health",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SandboxManager", "Health"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod sandbox_manager_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with SandboxManagerServer.
    #[async_trait]
    pub trait SandboxManager: std::marker::Send + std::marker::Sync + 'static {
        async fn acquire_lease(
            &self,
            request: tonic::Request<super::AcquireLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AcquireLeaseResponse>,
            tonic::Status,
        >;
        async fn release_lease(
            &self,
            request: tonic::Request<super::ReleaseLeaseRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReleaseLeaseResponse>,
            tonic::Status,
        >;
        async fn snapshot_sandbox(
            &self,
            request: tonic::Request<super::SnapshotRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SnapshotResponse>,
            tonic::Status,
        >;
        async fn health(
            &self,
            request: tonic::Request<super::SandboxHealthRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SandboxHealthResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct SandboxManagerServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> SandboxManagerServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for SandboxManagerServer<T>
    where
        T: SandboxManager,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.SandboxManager/AcquireLease" => {
                    #[allow(non_camel_case_types)]
                    struct AcquireLeaseSvc<T: SandboxManager>(pub Arc<T>);
                    impl<
                        T: SandboxManager,
                    > tonic::server::UnaryService<super::AcquireLeaseRequest>
                    for AcquireLeaseSvc<T> {
                        type Response = super::AcquireLeaseResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AcquireLeaseRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SandboxManager>::acquire_lease(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AcquireLeaseSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SandboxManager/ReleaseLease" => {
                    #[allow(non_camel_case_types)]
                    struct ReleaseLeaseSvc<T: SandboxManager>(pub Arc<T>);
                    impl<
                        T: SandboxManager,
                    > tonic::server::UnaryService<super::ReleaseLeaseRequest>
                    for ReleaseLeaseSvc<T> {
                        type Response = super::ReleaseLeaseResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReleaseLeaseRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SandboxManager>::release_lease(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ReleaseLeaseSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SandboxManager/SnapshotSandbox" => {
                    #[allow(non_camel_case_types)]
                    struct SnapshotSandboxSvc<T: SandboxManager>(pub Arc<T>);
                    impl<
                        T: SandboxManager,
                    > tonic::server::UnaryService<super::SnapshotRequest>
                    for SnapshotSandboxSvc<T> {
                        type Response = super::SnapshotResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SnapshotRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SandboxManager>::snapshot_sandbox(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SnapshotSandboxSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SandboxManager/Health" => {
                    #[allow(non_camel_case_types)]
                    struct HealthSvc<T: SandboxManager>(pub Arc<T>);
                    impl<
                        T: SandboxManager,
                    > tonic::server::UnaryService<super::SandboxHealthRequest>
                    for HealthSvc<T> {
                        type Response = super::SandboxHealthResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SandboxHealthRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SandboxManager>::health(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = HealthSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for SandboxManagerServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.SandboxManager";
    impl<T> tonic::server::NamedService for SandboxManagerServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod session_core_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct SessionCoreClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl SessionCoreClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> SessionCoreClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> SessionCoreClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            SessionCoreClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn create_thread(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateThreadResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/CreateThread",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "CreateThread"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn append_message(
            &mut self,
            request: impl tonic::IntoRequest<super::AppendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendMessageResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/AppendMessage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "AppendMessage"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn start_run(
            &mut self,
            request: impl tonic::IntoRequest<super::StartRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/StartRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "StartRun"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn complete_step(
            &mut self,
            request: impl tonic::IntoRequest<super::CompleteStepRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CompleteStepResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/CompleteStep",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "CompleteStep"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn reserve_tool_action(
            &mut self,
            request: impl tonic::IntoRequest<super::ReserveToolActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReserveToolActionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/ReserveToolAction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "ReserveToolAction"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn finalize_tool_action(
            &mut self,
            request: impl tonic::IntoRequest<super::FinalizeToolActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::FinalizeToolActionResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/FinalizeToolAction",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "FinalizeToolAction"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn save_checkpoint(
            &mut self,
            request: impl tonic::IntoRequest<super::SaveCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveCheckpointResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/SaveCheckpoint",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "SaveCheckpoint"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn replay_thread(
            &mut self,
            request: impl tonic::IntoRequest<super::ReplayThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::Event>>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/ReplayThread",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "ReplayThread"));
            self.inner.server_streaming(req, path, codec).await
        }
        pub async fn get_context_assembly(
            &mut self,
            request: impl tonic::IntoRequest<super::GetContextAssemblyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetContextAssemblyResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/GetContextAssembly",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "GetContextAssembly"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn compact_now(
            &mut self,
            request: impl tonic::IntoRequest<super::CompactNowRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CompactNowResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/CompactNow",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "CompactNow"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn upsert_agent_skill(
            &mut self,
            request: impl tonic::IntoRequest<super::UpsertAgentSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UpsertAgentSkillResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/UpsertAgentSkill",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "UpsertAgentSkill"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_agent_skills(
            &mut self,
            request: impl tonic::IntoRequest<super::ListAgentSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListAgentSkillsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/ListAgentSkills",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "ListAgentSkills"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_agent_skill_enabled(
            &mut self,
            request: impl tonic::IntoRequest<super::SetAgentSkillEnabledRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetAgentSkillEnabledResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/SetAgentSkillEnabled",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "SetAgentSkillEnabled"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_conversation(
            &mut self,
            request: impl tonic::IntoRequest<super::ListConversationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListConversationResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/ListConversation",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("model_plane.v1.SessionCore", "ListConversation"),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn list_threads(
            &mut self,
            request: impl tonic::IntoRequest<super::ListThreadsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/ListThreads",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "ListThreads"));
            self.inner.unary(req, path, codec).await
        }
        pub async fn set_run_mode(
            &mut self,
            request: impl tonic::IntoRequest<super::SetRunModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetRunModeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.SessionCore/SetRunMode",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("model_plane.v1.SessionCore", "SetRunMode"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod session_core_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with SessionCoreServer.
    #[async_trait]
    pub trait SessionCore: std::marker::Send + std::marker::Sync + 'static {
        async fn create_thread(
            &self,
            request: tonic::Request<super::CreateThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreateThreadResponse>,
            tonic::Status,
        >;
        async fn append_message(
            &self,
            request: tonic::Request<super::AppendMessageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::AppendMessageResponse>,
            tonic::Status,
        >;
        async fn start_run(
            &self,
            request: tonic::Request<super::StartRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartRunResponse>,
            tonic::Status,
        >;
        async fn complete_step(
            &self,
            request: tonic::Request<super::CompleteStepRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CompleteStepResponse>,
            tonic::Status,
        >;
        async fn reserve_tool_action(
            &self,
            request: tonic::Request<super::ReserveToolActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReserveToolActionResponse>,
            tonic::Status,
        >;
        async fn finalize_tool_action(
            &self,
            request: tonic::Request<super::FinalizeToolActionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::FinalizeToolActionResponse>,
            tonic::Status,
        >;
        async fn save_checkpoint(
            &self,
            request: tonic::Request<super::SaveCheckpointRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SaveCheckpointResponse>,
            tonic::Status,
        >;
        /// Server streaming response type for the ReplayThread method.
        type ReplayThreadStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::Event, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        async fn replay_thread(
            &self,
            request: tonic::Request<super::ReplayThreadRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::ReplayThreadStream>,
            tonic::Status,
        >;
        async fn get_context_assembly(
            &self,
            request: tonic::Request<super::GetContextAssemblyRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetContextAssemblyResponse>,
            tonic::Status,
        >;
        async fn compact_now(
            &self,
            request: tonic::Request<super::CompactNowRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CompactNowResponse>,
            tonic::Status,
        >;
        async fn upsert_agent_skill(
            &self,
            request: tonic::Request<super::UpsertAgentSkillRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UpsertAgentSkillResponse>,
            tonic::Status,
        >;
        async fn list_agent_skills(
            &self,
            request: tonic::Request<super::ListAgentSkillsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListAgentSkillsResponse>,
            tonic::Status,
        >;
        async fn set_agent_skill_enabled(
            &self,
            request: tonic::Request<super::SetAgentSkillEnabledRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetAgentSkillEnabledResponse>,
            tonic::Status,
        >;
        async fn list_conversation(
            &self,
            request: tonic::Request<super::ListConversationRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListConversationResponse>,
            tonic::Status,
        >;
        async fn list_threads(
            &self,
            request: tonic::Request<super::ListThreadsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListThreadsResponse>,
            tonic::Status,
        >;
        async fn set_run_mode(
            &self,
            request: tonic::Request<super::SetRunModeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SetRunModeResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct SessionCoreServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> SessionCoreServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for SessionCoreServer<T>
    where
        T: SessionCore,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.SessionCore/CreateThread" => {
                    #[allow(non_camel_case_types)]
                    struct CreateThreadSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::CreateThreadRequest>
                    for CreateThreadSvc<T> {
                        type Response = super::CreateThreadResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateThreadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::create_thread(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateThreadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/AppendMessage" => {
                    #[allow(non_camel_case_types)]
                    struct AppendMessageSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::AppendMessageRequest>
                    for AppendMessageSvc<T> {
                        type Response = super::AppendMessageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::AppendMessageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::append_message(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = AppendMessageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/StartRun" => {
                    #[allow(non_camel_case_types)]
                    struct StartRunSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::StartRunRequest>
                    for StartRunSvc<T> {
                        type Response = super::StartRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StartRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::start_run(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StartRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/CompleteStep" => {
                    #[allow(non_camel_case_types)]
                    struct CompleteStepSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::CompleteStepRequest>
                    for CompleteStepSvc<T> {
                        type Response = super::CompleteStepResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CompleteStepRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::complete_step(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CompleteStepSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/ReserveToolAction" => {
                    #[allow(non_camel_case_types)]
                    struct ReserveToolActionSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::ReserveToolActionRequest>
                    for ReserveToolActionSvc<T> {
                        type Response = super::ReserveToolActionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReserveToolActionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::reserve_tool_action(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ReserveToolActionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/FinalizeToolAction" => {
                    #[allow(non_camel_case_types)]
                    struct FinalizeToolActionSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::FinalizeToolActionRequest>
                    for FinalizeToolActionSvc<T> {
                        type Response = super::FinalizeToolActionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::FinalizeToolActionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::finalize_tool_action(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = FinalizeToolActionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/SaveCheckpoint" => {
                    #[allow(non_camel_case_types)]
                    struct SaveCheckpointSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::SaveCheckpointRequest>
                    for SaveCheckpointSvc<T> {
                        type Response = super::SaveCheckpointResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SaveCheckpointRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::save_checkpoint(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SaveCheckpointSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/ReplayThread" => {
                    #[allow(non_camel_case_types)]
                    struct ReplayThreadSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::ServerStreamingService<super::ReplayThreadRequest>
                    for ReplayThreadSvc<T> {
                        type Response = super::Event;
                        type ResponseStream = T::ReplayThreadStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReplayThreadRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::replay_thread(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ReplayThreadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/GetContextAssembly" => {
                    #[allow(non_camel_case_types)]
                    struct GetContextAssemblySvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::GetContextAssemblyRequest>
                    for GetContextAssemblySvc<T> {
                        type Response = super::GetContextAssemblyResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetContextAssemblyRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::get_context_assembly(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetContextAssemblySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/CompactNow" => {
                    #[allow(non_camel_case_types)]
                    struct CompactNowSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::CompactNowRequest>
                    for CompactNowSvc<T> {
                        type Response = super::CompactNowResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CompactNowRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::compact_now(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CompactNowSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/UpsertAgentSkill" => {
                    #[allow(non_camel_case_types)]
                    struct UpsertAgentSkillSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::UpsertAgentSkillRequest>
                    for UpsertAgentSkillSvc<T> {
                        type Response = super::UpsertAgentSkillResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpsertAgentSkillRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::upsert_agent_skill(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = UpsertAgentSkillSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/ListAgentSkills" => {
                    #[allow(non_camel_case_types)]
                    struct ListAgentSkillsSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::ListAgentSkillsRequest>
                    for ListAgentSkillsSvc<T> {
                        type Response = super::ListAgentSkillsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListAgentSkillsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::list_agent_skills(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListAgentSkillsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/SetAgentSkillEnabled" => {
                    #[allow(non_camel_case_types)]
                    struct SetAgentSkillEnabledSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::SetAgentSkillEnabledRequest>
                    for SetAgentSkillEnabledSvc<T> {
                        type Response = super::SetAgentSkillEnabledResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetAgentSkillEnabledRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::set_agent_skill_enabled(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetAgentSkillEnabledSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/ListConversation" => {
                    #[allow(non_camel_case_types)]
                    struct ListConversationSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::ListConversationRequest>
                    for ListConversationSvc<T> {
                        type Response = super::ListConversationResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListConversationRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::list_conversation(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListConversationSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/ListThreads" => {
                    #[allow(non_camel_case_types)]
                    struct ListThreadsSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::ListThreadsRequest>
                    for ListThreadsSvc<T> {
                        type Response = super::ListThreadsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListThreadsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::list_threads(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListThreadsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.SessionCore/SetRunMode" => {
                    #[allow(non_camel_case_types)]
                    struct SetRunModeSvc<T: SessionCore>(pub Arc<T>);
                    impl<
                        T: SessionCore,
                    > tonic::server::UnaryService<super::SetRunModeRequest>
                    for SetRunModeSvc<T> {
                        type Response = super::SetRunModeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SetRunModeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SessionCore>::set_run_mode(&inner, request).await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SetRunModeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for SessionCoreServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.SessionCore";
    impl<T> tonic::server::NamedService for SessionCoreServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
/// Generated client implementations.
pub mod managed_run_lifecycle_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct ManagedRunLifecycleClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl ManagedRunLifecycleClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> ManagedRunLifecycleClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ManagedRunLifecycleClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            ManagedRunLifecycleClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn start_managed_run(
            &mut self,
            request: impl tonic::IntoRequest<super::StartManagedRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartManagedRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ManagedRunLifecycle/StartManagedRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.ManagedRunLifecycle",
                        "StartManagedRun",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn record_terminal_outcome(
            &mut self,
            request: impl tonic::IntoRequest<super::RecordTerminalOutcomeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordTerminalOutcomeResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ManagedRunLifecycle/RecordTerminalOutcome",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.ManagedRunLifecycle",
                        "RecordTerminalOutcome",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        pub async fn heartbeat_managed_run(
            &mut self,
            request: impl tonic::IntoRequest<super::HeartbeatManagedRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::HeartbeatManagedRunResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/model_plane.v1.ManagedRunLifecycle/HeartbeatManagedRun",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "model_plane.v1.ManagedRunLifecycle",
                        "HeartbeatManagedRun",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod managed_run_lifecycle_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ManagedRunLifecycleServer.
    #[async_trait]
    pub trait ManagedRunLifecycle: std::marker::Send + std::marker::Sync + 'static {
        async fn start_managed_run(
            &self,
            request: tonic::Request<super::StartManagedRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::StartManagedRunResponse>,
            tonic::Status,
        >;
        async fn record_terminal_outcome(
            &self,
            request: tonic::Request<super::RecordTerminalOutcomeRequest>,
        ) -> std::result::Result<
            tonic::Response<super::RecordTerminalOutcomeResponse>,
            tonic::Status,
        >;
        async fn heartbeat_managed_run(
            &self,
            request: tonic::Request<super::HeartbeatManagedRunRequest>,
        ) -> std::result::Result<
            tonic::Response<super::HeartbeatManagedRunResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct ManagedRunLifecycleServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> ManagedRunLifecycleServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ManagedRunLifecycleServer<T>
    where
        T: ManagedRunLifecycle,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/model_plane.v1.ManagedRunLifecycle/StartManagedRun" => {
                    #[allow(non_camel_case_types)]
                    struct StartManagedRunSvc<T: ManagedRunLifecycle>(pub Arc<T>);
                    impl<
                        T: ManagedRunLifecycle,
                    > tonic::server::UnaryService<super::StartManagedRunRequest>
                    for StartManagedRunSvc<T> {
                        type Response = super::StartManagedRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StartManagedRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ManagedRunLifecycle>::start_managed_run(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StartManagedRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ManagedRunLifecycle/RecordTerminalOutcome" => {
                    #[allow(non_camel_case_types)]
                    struct RecordTerminalOutcomeSvc<T: ManagedRunLifecycle>(pub Arc<T>);
                    impl<
                        T: ManagedRunLifecycle,
                    > tonic::server::UnaryService<super::RecordTerminalOutcomeRequest>
                    for RecordTerminalOutcomeSvc<T> {
                        type Response = super::RecordTerminalOutcomeResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::RecordTerminalOutcomeRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ManagedRunLifecycle>::record_terminal_outcome(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = RecordTerminalOutcomeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/model_plane.v1.ManagedRunLifecycle/HeartbeatManagedRun" => {
                    #[allow(non_camel_case_types)]
                    struct HeartbeatManagedRunSvc<T: ManagedRunLifecycle>(pub Arc<T>);
                    impl<
                        T: ManagedRunLifecycle,
                    > tonic::server::UnaryService<super::HeartbeatManagedRunRequest>
                    for HeartbeatManagedRunSvc<T> {
                        type Response = super::HeartbeatManagedRunResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::HeartbeatManagedRunRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ManagedRunLifecycle>::heartbeat_managed_run(
                                        &inner,
                                        request,
                                    )
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = HeartbeatManagedRunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for ManagedRunLifecycleServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "model_plane.v1.ManagedRunLifecycle";
    impl<T> tonic::server::NamedService for ManagedRunLifecycleServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
