//! Deterministic visual evidence processing.
//!
//! Quarry owns capture and normalization of evidence, not visual reasoning. The
//! first implementation is a sidecar seam for OpenCV-style image processing so
//! the default Rust edge build does not link native OpenCV.

use std::collections::BTreeMap;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct VisualObservationInput {
    pub run_id: String,
    pub page_hash: String,
    pub step: u32,
    pub previous_png: Option<Vec<u8>>,
    pub current_png: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VisualPreprocessInput {
    pub document_id: String,
    pub image_png: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VisualPreprocessResult {
    pub clean_png: Vec<u8>,
    pub metrics: serde_json::Value,
    pub thumbnail_png: Option<Vec<u8>>,
    pub tiles: Option<serde_json::Value>,
    pub ocr_preprocessed_png: Option<Vec<u8>>,
    pub logo_candidate_png: Option<Vec<u8>>,
    pub rendered_palette: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct VisualProcessorOptions {
    pub max_regions: usize,
    pub diff: bool,
    pub screenshot_preprocessing: bool,
    pub thumbnail: bool,
    pub tiles: bool,
    pub ocr_preconditioning: bool,
    pub rendered_branding: bool,
}

impl Default for VisualProcessorOptions {
    fn default() -> Self {
        Self {
            max_regions: 32,
            diff: true,
            screenshot_preprocessing: true,
            thumbnail: true,
            tiles: true,
            ocr_preconditioning: false,
            rendered_branding: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualObservationArtifact {
    pub version: u8,
    pub backend: String,
    pub step: u32,
    pub previous_available: bool,
    pub changed: bool,
    pub change_ratio: f32,
    #[serde(default)]
    pub regions: Vec<VisualRegion>,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub related_artifacts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualChangeArtifact {
    pub version: u8,
    pub backend: String,
    pub step: u32,
    pub previous_available: bool,
    pub changed: bool,
    pub change_ratio: f32,
    #[serde(default)]
    pub regions: Vec<VisualRegion>,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_artifact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VisualObservationResult {
    pub artifact: VisualObservationArtifact,
    pub annotated_png: Option<Vec<u8>>,
    pub clean_png: Option<Vec<u8>>,
    pub thumbnail_png: Option<Vec<u8>>,
    pub tiles: Option<serde_json::Value>,
    pub ocr_preprocessed_png: Option<Vec<u8>>,
    pub logo_candidate_png: Option<Vec<u8>>,
    pub rendered_palette: Option<serde_json::Value>,
}

#[async_trait]
pub trait VisualObservationProcessor: Send + Sync {
    async fn observe(&self, input: VisualObservationInput)
        -> QuarryResult<VisualObservationResult>;

    async fn preprocess_page_image(
        &self,
        input: VisualPreprocessInput,
    ) -> QuarryResult<VisualPreprocessResult> {
        Ok(VisualPreprocessResult {
            clean_png: input.image_png,
            metrics: serde_json::Value::Null,
            thumbnail_png: None,
            tiles: None,
            ocr_preprocessed_png: None,
            logo_candidate_png: None,
            rendered_palette: None,
        })
    }
}

#[derive(Clone)]
pub struct SidecarVisualProcessor {
    client: reqwest::Client,
    endpoint: String,
    preprocess_endpoint: String,
    options: VisualProcessorOptions,
}

impl SidecarVisualProcessor {
    pub fn new(base_url: impl Into<String>) -> QuarryResult<Self> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "vision sidecar URL cannot be empty",
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("vision client: {e}")))?;
        Ok(Self {
            client,
            endpoint: format!("{}/v1/visual/observe", base_url.trim_end_matches('/')),
            preprocess_endpoint: format!("{}/v1/visual/preprocess", base_url.trim_end_matches('/')),
            options: VisualProcessorOptions::default(),
        })
    }

    pub fn with_options(mut self, options: VisualProcessorOptions) -> Self {
        self.options = options;
        self
    }
}

#[async_trait]
impl VisualObservationProcessor for SidecarVisualProcessor {
    async fn observe(
        &self,
        input: VisualObservationInput,
    ) -> QuarryResult<VisualObservationResult> {
        let VisualObservationInput {
            run_id,
            page_hash,
            step,
            previous_png,
            current_png,
        } = input;
        let request = SidecarVisualRequest {
            run_id,
            page_hash,
            step,
            previous_png_b64: previous_png.as_deref().map(|bytes| B64.encode(bytes)),
            current_png_b64: B64.encode(&current_png),
            max_regions: self.options.max_regions.clamp(1, 128),
            operations: SidecarVisualOperations::from(&self.options),
        };

        let response = self
            .client
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, format!("vision sidecar: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("vision sidecar returned {}: {}", status.as_u16(), body),
            ));
        }

        let response: SidecarVisualResponse = response.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("vision sidecar decode: {e}"),
            )
        })?;
        let annotated_png = match response.annotated_png_b64 {
            Some(encoded) if !encoded.is_empty() => Some(B64.decode(encoded).map_err(|e| {
                QuarryError::new(
                    ErrorCode::DriverFailed,
                    format!("vision sidecar annotated image is invalid base64: {e}"),
                )
            })?),
            _ => None,
        };

        Ok(VisualObservationResult {
            artifact: VisualObservationArtifact {
                version: response.version.unwrap_or(1),
                backend: response
                    .backend
                    .unwrap_or_else(|| "opencv5-sidecar".to_string()),
                step: response.step.unwrap_or(step),
                previous_available: response.previous_available,
                changed: response.changed,
                change_ratio: response.change_ratio,
                regions: response.regions,
                metrics: response.metrics.unwrap_or(serde_json::Value::Null),
                annotated_artifact_id: None,
                change_artifact_id: None,
                related_artifacts: BTreeMap::new(),
            },
            annotated_png,
            clean_png: decode_optional_png(response.clean_png_b64, "clean image")?,
            thumbnail_png: decode_optional_png(response.thumbnail_png_b64, "thumbnail")?,
            tiles: response.tiles,
            ocr_preprocessed_png: decode_optional_png(
                response.ocr_preprocessed_png_b64,
                "ocr preprocessed image",
            )?,
            logo_candidate_png: decode_optional_png(
                response.logo_candidate_png_b64,
                "logo candidate image",
            )?,
            rendered_palette: response.rendered_palette,
        })
    }

    async fn preprocess_page_image(
        &self,
        input: VisualPreprocessInput,
    ) -> QuarryResult<VisualPreprocessResult> {
        let request = SidecarPreprocessRequest {
            document_id: input.document_id,
            image_png_b64: B64.encode(&input.image_png),
            operations: SidecarVisualOperations::from(&self.options),
        };

        let response = self
            .client
            .post(&self.preprocess_endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                QuarryError::new(ErrorCode::DriverFailed, format!("vision sidecar: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                format!("vision sidecar returned {}: {}", status.as_u16(), body),
            ));
        }

        let response: SidecarPreprocessResponse = response.json().await.map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("vision sidecar decode: {e}"),
            )
        })?;

        Ok(VisualPreprocessResult {
            clean_png: decode_required_png(response.clean_png_b64, "clean image")?,
            metrics: response.metrics.unwrap_or(serde_json::Value::Null),
            thumbnail_png: decode_optional_png(response.thumbnail_png_b64, "thumbnail")?,
            tiles: response.tiles,
            ocr_preprocessed_png: decode_optional_png(
                response.ocr_preprocessed_png_b64,
                "ocr preprocessed image",
            )?,
            logo_candidate_png: decode_optional_png(
                response.logo_candidate_png_b64,
                "logo candidate image",
            )?,
            rendered_palette: response.rendered_palette,
        })
    }
}

#[derive(Debug, Serialize)]
struct SidecarVisualRequest {
    run_id: String,
    page_hash: String,
    step: u32,
    previous_png_b64: Option<String>,
    current_png_b64: String,
    max_regions: usize,
    operations: SidecarVisualOperations,
}

#[derive(Debug, Serialize)]
struct SidecarPreprocessRequest {
    document_id: String,
    image_png_b64: String,
    operations: SidecarVisualOperations,
}

#[derive(Debug, Clone, Serialize)]
struct SidecarVisualOperations {
    diff: bool,
    screenshot_preprocessing: bool,
    thumbnail: bool,
    tiles: bool,
    ocr_preconditioning: bool,
    rendered_branding: bool,
}

impl From<&VisualProcessorOptions> for SidecarVisualOperations {
    fn from(value: &VisualProcessorOptions) -> Self {
        Self {
            diff: value.diff,
            screenshot_preprocessing: value.screenshot_preprocessing,
            thumbnail: value.thumbnail,
            tiles: value.tiles,
            ocr_preconditioning: value.ocr_preconditioning,
            rendered_branding: value.rendered_branding,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SidecarVisualResponse {
    #[serde(default)]
    version: Option<u8>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    step: Option<u32>,
    #[serde(default)]
    previous_available: bool,
    changed: bool,
    change_ratio: f32,
    #[serde(default)]
    regions: Vec<VisualRegion>,
    #[serde(default)]
    metrics: Option<serde_json::Value>,
    #[serde(default)]
    annotated_png_b64: Option<String>,
    #[serde(default)]
    clean_png_b64: Option<String>,
    #[serde(default)]
    thumbnail_png_b64: Option<String>,
    #[serde(default)]
    tiles: Option<serde_json::Value>,
    #[serde(default)]
    ocr_preprocessed_png_b64: Option<String>,
    #[serde(default)]
    logo_candidate_png_b64: Option<String>,
    #[serde(default)]
    rendered_palette: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SidecarPreprocessResponse {
    clean_png_b64: String,
    #[serde(default)]
    metrics: Option<serde_json::Value>,
    #[serde(default)]
    thumbnail_png_b64: Option<String>,
    #[serde(default)]
    tiles: Option<serde_json::Value>,
    #[serde(default)]
    ocr_preprocessed_png_b64: Option<String>,
    #[serde(default)]
    logo_candidate_png_b64: Option<String>,
    #[serde(default)]
    rendered_palette: Option<serde_json::Value>,
}

fn decode_required_png(encoded: String, label: &str) -> QuarryResult<Vec<u8>> {
    B64.decode(encoded).map_err(|e| {
        QuarryError::new(
            ErrorCode::DriverFailed,
            format!("vision sidecar {label} is invalid base64: {e}"),
        )
    })
}

fn decode_optional_png(encoded: Option<String>, label: &str) -> QuarryResult<Option<Vec<u8>>> {
    match encoded {
        Some(encoded) if !encoded.is_empty() => Ok(Some(decode_required_png(encoded, label)?)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sidecar_posts_base64_pngs_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/visual/observe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "backend": "opencv5-sidecar",
                "step": 2,
                "previous_available": true,
                "changed": true,
                "change_ratio": 0.25,
                "regions": [{ "x": 1, "y": 2, "width": 3, "height": 4, "score": 0.9 }],
                "metrics": { "threshold": 18 },
                "annotated_png_b64": B64.encode(b"annotated"),
                "clean_png_b64": B64.encode(b"clean"),
                "thumbnail_png_b64": B64.encode(b"thumb"),
                "tiles": { "tiles": [{ "x": 0, "y": 0 }] },
                "ocr_preprocessed_png_b64": B64.encode(b"ocr"),
                "logo_candidate_png_b64": B64.encode(b"logo"),
                "rendered_palette": { "colors": [{ "hex": "#102030" }] }
            })))
            .mount(&server)
            .await;

        let processor = SidecarVisualProcessor::new(server.uri()).unwrap();
        let result = processor
            .observe(VisualObservationInput {
                run_id: "run_1".into(),
                page_hash: "blake3:x".into(),
                step: 2,
                previous_png: Some(b"prev".to_vec()),
                current_png: b"curr".to_vec(),
            })
            .await
            .unwrap();

        assert!(result.artifact.changed);
        assert_eq!(result.artifact.change_ratio, 0.25);
        assert_eq!(result.artifact.regions[0].width, 3);
        assert_eq!(result.annotated_png.as_deref(), Some(&b"annotated"[..]));
        assert_eq!(result.clean_png.as_deref(), Some(&b"clean"[..]));
        assert_eq!(result.thumbnail_png.as_deref(), Some(&b"thumb"[..]));
        assert_eq!(result.ocr_preprocessed_png.as_deref(), Some(&b"ocr"[..]));
        assert_eq!(result.logo_candidate_png.as_deref(), Some(&b"logo"[..]));
        assert!(result.tiles.is_some());
        assert!(result.rendered_palette.is_some());
    }

    #[tokio::test]
    async fn sidecar_preprocess_decodes_clean_page_image() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/visual/preprocess"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "backend": "opencv5-sidecar",
                "clean_png_b64": B64.encode(b"clean-page"),
                "metrics": { "clean_width": 100 },
                "thumbnail_png_b64": B64.encode(b"thumb"),
                "tiles": { "tile_size": 512 }
            })))
            .mount(&server)
            .await;

        let processor = SidecarVisualProcessor::new(server.uri()).unwrap();
        let result = processor
            .preprocess_page_image(VisualPreprocessInput {
                document_id: "doc_1".into(),
                image_png: b"page".to_vec(),
            })
            .await
            .unwrap();

        assert_eq!(result.clean_png, b"clean-page");
        assert_eq!(result.thumbnail_png.as_deref(), Some(&b"thumb"[..]));
        assert_eq!(result.metrics["clean_width"], 100);
        assert!(result.tiles.is_some());
    }

    #[test]
    fn sidecar_rejects_empty_url() {
        let err = match SidecarVisualProcessor::new(" ") {
            Ok(_) => panic!("empty sidecar URL should be rejected"),
            Err(err) => err,
        };
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
