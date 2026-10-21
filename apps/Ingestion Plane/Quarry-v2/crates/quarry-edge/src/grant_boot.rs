//! Boot policy for the BrowserBroker grant validator (WS1.A2).
//!
//! `main` asks [`decide`] once, before it builds anything, which validator the
//! environment calls for or why the edge must not start. The rules are kept
//! here, apart from `main`, so a test can drive every combination without
//! booting a process:
//!
//! - **Production requires grants.** `ENVIRONMENT=prod|production` implies
//!   `require_browser_grants`, so an edge with no validator endpoint refuses to
//!   boot (the older generic gate).
//! - **Production needs the gRPC validator.** `HttpGrantValidator` posts to
//!   `/v1/broker/grants/{id}/validate`, a route no Model Plane service serves,
//!   so an HTTP-only production edge would fail every run or, worse, look
//!   configured. It is refused; outside production it stays as a development
//!   compatibility shim.
//! - **The gRPC validator needs service tokens.** The broker authorizes
//!   `ValidateGrant` by a verified token for `browser:read` on audience
//!   `browser-broker`, scoped to an organization, so a gRPC endpoint without
//!   `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` (the credential Auth Core mints
//!   those tokens against) could never validate anything. The one exception is
//!   the explicit cross-plane dev bypass with a static development bearer,
//!   which production refuses elsewhere.
//! - **Not in production**, with no endpoint and no requirement, grants are
//!   simply not enforced (`Noop`): a permissive local stack. That is logged.

use std::fmt;

/// Which validator the edge builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorChoice {
    /// `GrpcGrantValidator`, with service tokens (or the dev bearer).
    Grpc,
    /// `HttpGrantValidator`, the development compatibility shim.
    Http,
    /// No endpoint and none required: every grant is accepted.
    Noop,
}

/// What the environment says about grant validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantBootInputs {
    /// `ENVIRONMENT` / `APP_ENV` is `prod` or `production`.
    pub production: bool,
    /// `QUARRY_EDGE__REQUIRE_BROWSER_GRANTS` or production.
    pub require_grants: bool,
    /// `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` is set and non-blank.
    pub http_configured: bool,
    /// `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` is set and non-blank.
    pub grpc_configured: bool,
    /// The binary was built with the `grpc` feature.
    pub grpc_compiled: bool,
    /// `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` is set and non-blank.
    pub service_tokens_configured: bool,
    /// `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS`.
    pub dev_bypass: bool,
}

/// Why the edge must not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantBootRefusal {
    /// A gRPC endpoint is configured in a binary without the `grpc` feature.
    GrpcNotCompiled,
    /// Grants are required and no validator endpoint is configured.
    NoValidatorWhenRequired,
    /// Production with only the HTTP shim.
    HttpOnlyInProduction,
    /// A gRPC endpoint without the credential to mint broker tokens.
    GrpcWithoutServiceTokens,
}

impl fmt::Display for GrantBootRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::GrpcNotCompiled => {
                "QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL requires the grpc feature"
            }
            Self::NoValidatorWhenRequired => {
                "browser grants are required but no BrowserBroker validation endpoint is configured"
            }
            Self::HttpOnlyInProduction => {
                "production refuses the HTTP-only browser grant validator: no Model Plane service serves \
                 its route; set QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL to the BrowserBroker"
            }
            Self::GrpcWithoutServiceTokens => {
                "QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL needs QUARRY_EDGE__QUARRY_SERVICE_API_KEY: \
                 the BrowserBroker accepts only a service token minted for the run's organization"
            }
        })
    }
}

impl std::error::Error for GrantBootRefusal {}

/// Which validator to build, or why not to boot. Pure: no I/O, no environment.
pub fn decide(inputs: GrantBootInputs) -> Result<ValidatorChoice, GrantBootRefusal> {
    if inputs.grpc_configured && !inputs.grpc_compiled {
        return Err(GrantBootRefusal::GrpcNotCompiled);
    }
    if inputs.require_grants && !inputs.http_configured && !inputs.grpc_configured {
        return Err(GrantBootRefusal::NoValidatorWhenRequired);
    }
    if inputs.production && inputs.http_configured && !inputs.grpc_configured {
        return Err(GrantBootRefusal::HttpOnlyInProduction);
    }
    // The dev bypass never counts in production (`main` refuses to start with
    // both); this keeps the rule from depending on that ordering.
    let static_dev_bearer = inputs.dev_bypass && !inputs.production;
    if inputs.grpc_configured && !inputs.service_tokens_configured && !static_dev_bearer {
        return Err(GrantBootRefusal::GrpcWithoutServiceTokens);
    }
    Ok(if inputs.grpc_configured {
        ValidatorChoice::Grpc
    } else if inputs.http_configured {
        ValidatorChoice::Http
    } else {
        ValidatorChoice::Noop
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production() -> GrantBootInputs {
        GrantBootInputs {
            production: true,
            require_grants: true,
            http_configured: false,
            grpc_configured: true,
            grpc_compiled: true,
            service_tokens_configured: true,
            dev_bypass: false,
        }
    }

    fn development() -> GrantBootInputs {
        GrantBootInputs {
            production: false,
            require_grants: false,
            http_configured: false,
            grpc_configured: false,
            grpc_compiled: true,
            service_tokens_configured: false,
            dev_bypass: false,
        }
    }

    #[test]
    fn production_with_the_grpc_validator_and_service_tokens_boots() {
        assert_eq!(decide(production()), Ok(ValidatorChoice::Grpc));
        // The HTTP shim next to it changes nothing: gRPC is the one built.
        assert_eq!(
            decide(GrantBootInputs {
                http_configured: true,
                ..production()
            }),
            Ok(ValidatorChoice::Grpc)
        );
    }

    #[test]
    fn production_refuses_the_http_only_validator() {
        let refused = decide(GrantBootInputs {
            http_configured: true,
            grpc_configured: false,
            ..production()
        });
        assert_eq!(refused, Err(GrantBootRefusal::HttpOnlyInProduction));
        let text = refused.unwrap_err().to_string();
        assert!(text.contains("QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL"), "{text}");
    }

    #[test]
    fn production_without_any_validator_endpoint_refuses() {
        assert_eq!(
            decide(GrantBootInputs {
                grpc_configured: false,
                ..production()
            }),
            Err(GrantBootRefusal::NoValidatorWhenRequired)
        );
    }

    #[test]
    fn a_grpc_validator_without_service_tokens_refuses_in_every_environment() {
        for production in [true, false] {
            let refused = decide(GrantBootInputs {
                production,
                service_tokens_configured: false,
                ..GrantBootInputs {
                    grpc_configured: true,
                    ..development()
                }
            });
            assert_eq!(
                refused,
                Err(GrantBootRefusal::GrpcWithoutServiceTokens),
                "production={production}"
            );
        }
    }

    #[test]
    fn the_dev_bypass_keeps_a_static_bearer_validator_possible_outside_production_only() {
        let dev = GrantBootInputs {
            grpc_configured: true,
            dev_bypass: true,
            ..development()
        };
        assert_eq!(decide(dev), Ok(ValidatorChoice::Grpc));
        assert_eq!(
            decide(GrantBootInputs {
                production: true,
                require_grants: true,
                ..dev
            }),
            Err(GrantBootRefusal::GrpcWithoutServiceTokens),
            "production never accepts the bypass as a credential"
        );
    }

    #[test]
    fn a_grpc_endpoint_in_a_binary_without_the_feature_refuses() {
        assert_eq!(
            decide(GrantBootInputs {
                grpc_compiled: false,
                ..production()
            }),
            Err(GrantBootRefusal::GrpcNotCompiled)
        );
    }

    #[test]
    fn outside_production_the_http_shim_and_the_noop_validator_stay_available() {
        assert_eq!(decide(development()), Ok(ValidatorChoice::Noop));
        assert_eq!(
            decide(GrantBootInputs {
                http_configured: true,
                ..development()
            }),
            Ok(ValidatorChoice::Http)
        );
        // Requiring grants outside production still needs an endpoint.
        assert_eq!(
            decide(GrantBootInputs {
                require_grants: true,
                ..development()
            }),
            Err(GrantBootRefusal::NoValidatorWhenRequired)
        );
    }
}
