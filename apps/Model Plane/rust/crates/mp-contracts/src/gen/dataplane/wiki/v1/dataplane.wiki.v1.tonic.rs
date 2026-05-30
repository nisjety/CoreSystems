// @generated
/// Generated client implementations.
pub mod wiki_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    ///
    #[derive(Debug, Clone)]
    pub struct WikiServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl WikiServiceClient<tonic::transport::Channel> {
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
    impl<T> WikiServiceClient<T>
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
        ) -> WikiServiceClient<InterceptedService<T, F>>
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
            WikiServiceClient::new(InterceptedService::new(inner, interceptor))
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
        ///
        pub async fn get_page(
            &mut self,
            request: impl tonic::IntoRequest<super::GetPageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPageResponse>,
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
                "/dataplane.wiki.v1.WikiService/GetPage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("dataplane.wiki.v1.WikiService", "GetPage"));
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn get_page_by_path(
            &mut self,
            request: impl tonic::IntoRequest<super::GetPageByPathRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPageByPathResponse>,
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
                "/dataplane.wiki.v1.WikiService/GetPageByPath",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "GetPageByPath"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn list_page_versions(
            &mut self,
            request: impl tonic::IntoRequest<super::ListPageVersionsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPageVersionsResponse>,
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
                "/dataplane.wiki.v1.WikiService/ListPageVersions",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "ListPageVersions"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn get_page_sources(
            &mut self,
            request: impl tonic::IntoRequest<super::GetPageSourcesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPageSourcesResponse>,
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
                "/dataplane.wiki.v1.WikiService/GetPageSources",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "GetPageSources"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn list_maintenance_issues(
            &mut self,
            request: impl tonic::IntoRequest<super::ListMaintenanceIssuesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMaintenanceIssuesResponse>,
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
                "/dataplane.wiki.v1.WikiService/ListMaintenanceIssues",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "dataplane.wiki.v1.WikiService",
                        "ListMaintenanceIssues",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn get_backlinks(
            &mut self,
            request: impl tonic::IntoRequest<super::GetBacklinksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBacklinksResponse>,
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
                "/dataplane.wiki.v1.WikiService/GetBacklinks",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "GetBacklinks"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn create_page(
            &mut self,
            request: impl tonic::IntoRequest<super::CreatePageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreatePageResponse>,
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
                "/dataplane.wiki.v1.WikiService/CreatePage",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("dataplane.wiki.v1.WikiService", "CreatePage"));
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn update_page_version(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdatePageVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UpdatePageVersionResponse>,
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
                "/dataplane.wiki.v1.WikiService/UpdatePageVersion",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "UpdatePageVersion"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn submit_proposal(
            &mut self,
            request: impl tonic::IntoRequest<super::SubmitProposalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SubmitProposalResponse>,
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
                "/dataplane.wiki.v1.WikiService/SubmitProposal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "SubmitProposal"),
                );
            self.inner.unary(req, path, codec).await
        }
        ///
        pub async fn review_proposal(
            &mut self,
            request: impl tonic::IntoRequest<super::ReviewProposalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReviewProposalResponse>,
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
                "/dataplane.wiki.v1.WikiService/ReviewProposal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("dataplane.wiki.v1.WikiService", "ReviewProposal"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod wiki_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with WikiServiceServer.
    #[async_trait]
    pub trait WikiService: std::marker::Send + std::marker::Sync + 'static {
        ///
        async fn get_page(
            &self,
            request: tonic::Request<super::GetPageRequest>,
        ) -> std::result::Result<tonic::Response<super::GetPageResponse>, tonic::Status>;
        ///
        async fn get_page_by_path(
            &self,
            request: tonic::Request<super::GetPageByPathRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPageByPathResponse>,
            tonic::Status,
        >;
        ///
        async fn list_page_versions(
            &self,
            request: tonic::Request<super::ListPageVersionsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListPageVersionsResponse>,
            tonic::Status,
        >;
        ///
        async fn get_page_sources(
            &self,
            request: tonic::Request<super::GetPageSourcesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetPageSourcesResponse>,
            tonic::Status,
        >;
        ///
        async fn list_maintenance_issues(
            &self,
            request: tonic::Request<super::ListMaintenanceIssuesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListMaintenanceIssuesResponse>,
            tonic::Status,
        >;
        ///
        async fn get_backlinks(
            &self,
            request: tonic::Request<super::GetBacklinksRequest>,
        ) -> std::result::Result<
            tonic::Response<super::GetBacklinksResponse>,
            tonic::Status,
        >;
        ///
        async fn create_page(
            &self,
            request: tonic::Request<super::CreatePageRequest>,
        ) -> std::result::Result<
            tonic::Response<super::CreatePageResponse>,
            tonic::Status,
        >;
        ///
        async fn update_page_version(
            &self,
            request: tonic::Request<super::UpdatePageVersionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::UpdatePageVersionResponse>,
            tonic::Status,
        >;
        ///
        async fn submit_proposal(
            &self,
            request: tonic::Request<super::SubmitProposalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::SubmitProposalResponse>,
            tonic::Status,
        >;
        ///
        async fn review_proposal(
            &self,
            request: tonic::Request<super::ReviewProposalRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ReviewProposalResponse>,
            tonic::Status,
        >;
    }
    ///
    #[derive(Debug)]
    pub struct WikiServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> WikiServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for WikiServiceServer<T>
    where
        T: WikiService,
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
                "/dataplane.wiki.v1.WikiService/GetPage" => {
                    #[allow(non_camel_case_types)]
                    struct GetPageSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::GetPageRequest>
                    for GetPageSvc<T> {
                        type Response = super::GetPageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetPageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::get_page(&inner, request).await
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
                        let method = GetPageSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/GetPageByPath" => {
                    #[allow(non_camel_case_types)]
                    struct GetPageByPathSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::GetPageByPathRequest>
                    for GetPageByPathSvc<T> {
                        type Response = super::GetPageByPathResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetPageByPathRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::get_page_by_path(&inner, request).await
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
                        let method = GetPageByPathSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/ListPageVersions" => {
                    #[allow(non_camel_case_types)]
                    struct ListPageVersionsSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::ListPageVersionsRequest>
                    for ListPageVersionsSvc<T> {
                        type Response = super::ListPageVersionsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListPageVersionsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::list_page_versions(&inner, request)
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
                        let method = ListPageVersionsSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/GetPageSources" => {
                    #[allow(non_camel_case_types)]
                    struct GetPageSourcesSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::GetPageSourcesRequest>
                    for GetPageSourcesSvc<T> {
                        type Response = super::GetPageSourcesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetPageSourcesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::get_page_sources(&inner, request).await
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
                        let method = GetPageSourcesSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/ListMaintenanceIssues" => {
                    #[allow(non_camel_case_types)]
                    struct ListMaintenanceIssuesSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::ListMaintenanceIssuesRequest>
                    for ListMaintenanceIssuesSvc<T> {
                        type Response = super::ListMaintenanceIssuesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListMaintenanceIssuesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::list_maintenance_issues(&inner, request)
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
                        let method = ListMaintenanceIssuesSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/GetBacklinks" => {
                    #[allow(non_camel_case_types)]
                    struct GetBacklinksSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::GetBacklinksRequest>
                    for GetBacklinksSvc<T> {
                        type Response = super::GetBacklinksResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetBacklinksRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::get_backlinks(&inner, request).await
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
                        let method = GetBacklinksSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/CreatePage" => {
                    #[allow(non_camel_case_types)]
                    struct CreatePageSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::CreatePageRequest>
                    for CreatePageSvc<T> {
                        type Response = super::CreatePageResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreatePageRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::create_page(&inner, request).await
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
                        let method = CreatePageSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/UpdatePageVersion" => {
                    #[allow(non_camel_case_types)]
                    struct UpdatePageVersionSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::UpdatePageVersionRequest>
                    for UpdatePageVersionSvc<T> {
                        type Response = super::UpdatePageVersionResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdatePageVersionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::update_page_version(&inner, request)
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
                        let method = UpdatePageVersionSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/SubmitProposal" => {
                    #[allow(non_camel_case_types)]
                    struct SubmitProposalSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::SubmitProposalRequest>
                    for SubmitProposalSvc<T> {
                        type Response = super::SubmitProposalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SubmitProposalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::submit_proposal(&inner, request).await
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
                        let method = SubmitProposalSvc(inner);
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
                "/dataplane.wiki.v1.WikiService/ReviewProposal" => {
                    #[allow(non_camel_case_types)]
                    struct ReviewProposalSvc<T: WikiService>(pub Arc<T>);
                    impl<
                        T: WikiService,
                    > tonic::server::UnaryService<super::ReviewProposalRequest>
                    for ReviewProposalSvc<T> {
                        type Response = super::ReviewProposalResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ReviewProposalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as WikiService>::review_proposal(&inner, request).await
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
                        let method = ReviewProposalSvc(inner);
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
    impl<T> Clone for WikiServiceServer<T> {
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
    pub const SERVICE_NAME: &str = "dataplane.wiki.v1.WikiService";
    impl<T> tonic::server::NamedService for WikiServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
