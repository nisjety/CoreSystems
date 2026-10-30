//! Opt-in ingestion generation tags for raw objects and manifest sidecars.

use std::sync::Arc;

use uuid::Uuid;

use crate::{GcsError, ObjectMeta, RawSink, Result};

/// Adds a caller-selected, nonsecret ingestion generation to every write.
/// Keys are isolated by generation; bodies and content types are unchanged.
/// Backing sinks use enriched metadata for objects and sidecars.
pub struct GenerationSink {
    inner: Arc<dyn RawSink>,
    generation: String,
    namespace: &'static str,
    source_system: &'static str,
}

impl GenerationSink {
    /// Validate the explicit generation and store its canonical UUID form.
    pub fn new(inner: Arc<dyn RawSink>, generation: &str) -> Result<Self> {
        Self::with_namespace(inner, generation, "v2", "xero_v2")
    }

    /// Use the natural namespace for the main Xero ingestion generation.
    pub fn new_main(inner: Arc<dyn RawSink>, generation: &str) -> Result<Self> {
        Self::with_namespace(inner, generation, "xero", "xero")
    }

    fn with_namespace(
        inner: Arc<dyn RawSink>,
        generation: &str,
        namespace: &'static str,
        source_system: &'static str,
    ) -> Result<Self> {
        let generation = Uuid::parse_str(generation)
            .map_err(|_| GcsError::Config("ingestion generation must be a valid UUID".into()))?
            .to_string();
        Ok(Self {
            inner,
            generation,
            namespace,
            source_system,
        })
    }

    /// Validate an original Xero key and isolate it from legacy notifications.
    pub fn generation_key(&self, key: &str) -> Result<String> {
        let allowed_root = key.starts_with("raw/xero/") || key.starts_with("_manifests/xero/");
        if !allowed_root
            || key.contains('\\')
            || key.chars().any(char::is_control)
            || key.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(GcsError::Config(
                "generation sink requires a safe raw/xero/ or _manifests/xero/ object key".into(),
            ));
        }
        Ok(format!("{}/{}/{key}", self.namespace, self.generation))
    }

    fn tagged_meta(&self, meta: &ObjectMeta) -> ObjectMeta {
        ObjectMeta(
            meta.0
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .chain([
                    ("x-source-system".into(), self.source_system.into()),
                    ("x-ingestion-generation".into(), self.generation.clone()),
                ])
                .collect(),
        )
    }
}

#[async_trait::async_trait]
impl RawSink for GenerationSink {
    async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> Result<()> {
        let key = self.generation_key(key)?;
        let meta = self.tagged_meta(meta);
        self.inner.put_raw(&key, body, &meta).await
    }

    async fn put_raw_typed(
        &self,
        key: &str,
        body: &[u8],
        meta: &ObjectMeta,
        content_type: &str,
    ) -> Result<()> {
        let key = self.generation_key(key)?;
        let meta = self.tagged_meta(meta);
        self.inner
            .put_raw_typed(&key, body, &meta, content_type)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use chrono::NaiveDate;
    use uuid::Uuid;

    use super::*;
    use crate::{manifest_key, sidecar_key, write_manifest, GcsError, LocalDirSink, RunManifest};

    const GENERATION: &str = "550e8400-e29b-41d4-a716-446655440000";
    const UNSAFE_KEYS: &[&str] = &[
        "",
        "raw/xero",
        "raw/xero/",
        "_manifests/xero/",
        "/raw/xero/file.json",
        "C:\\raw\\xero\\file.json",
        "raw/xero/../file.json",
        "raw/xero/a/../../file.json",
        "_manifests/xero/../file.json",
        "raw/xero/./file.json",
        "raw/xero//file.json",
        "_manifests/xero//file.json",
        "raw/xero/a\\..\\file.json",
        "raw/xero/file.json\0",
        "raw/xero/file.json\n",
        "other/raw/xero/file.json",
        "v2/550e8400-e29b-41d4-a716-446655440000/raw/xero/file.json",
    ];

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedWrite {
        key: String,
        body: Vec<u8>,
        meta: ObjectMeta,
        content_type: Option<String>,
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<RecordedWrite>>);

    impl RecordingSink {
        fn record(&self, key: &str, body: &[u8], meta: &ObjectMeta, content_type: Option<&str>) {
            self.0.lock().unwrap().push(RecordedWrite {
                key: key.into(),
                body: body.into(),
                meta: meta.clone(),
                content_type: content_type.map(str::to_string),
            });
        }

        fn writes(&self) -> Vec<RecordedWrite> {
            self.0.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl RawSink for RecordingSink {
        async fn put_raw(&self, key: &str, body: &[u8], meta: &ObjectMeta) -> Result<()> {
            self.record(key, body, meta, None);
            Ok(())
        }

        async fn put_raw_typed(
            &self,
            key: &str,
            body: &[u8],
            meta: &ObjectMeta,
            content_type: &str,
        ) -> Result<()> {
            self.record(key, body, meta, Some(content_type));
            Ok(())
        }
    }

    fn sample_meta() -> ObjectMeta {
        ObjectMeta(BTreeMap::from([
            ("x-vendor".into(), "xero".into()),
            ("x-tenant-id".into(), "tenant".into()),
            ("x-endpoint".into(), "invoices".into()),
            ("x-run-id".into(), "a-separate-run-id".into()),
            ("x-page".into(), "1".into()),
            ("custom-field".into(), "unchanged".into()),
        ]))
    }

    fn assert_tagged(tagged: &ObjectMeta, original: &ObjectMeta) {
        assert_eq!(tagged.get("x-source-system"), Some("xero_v2"));
        assert_eq!(tagged.get("x-ingestion-generation"), Some(GENERATION));
        let retained: Vec<_> = original
            .0
            .iter()
            .filter(|(key, _)| {
                !matches!(key.as_str(), "x-source-system" | "x-ingestion-generation")
            })
            .collect();
        assert_eq!(tagged.len(), retained.len() + 2);
        for (key, value) in retained {
            assert_eq!(tagged.get(key), Some(value.as_str()));
        }
    }

    async fn read_sidecar(root: &std::path::Path, key: &str) -> ObjectMeta {
        let body = tokio::fs::read(root.join(sidecar_key(key))).await.unwrap();
        ObjectMeta(serde_json::from_slice(&body).unwrap())
    }

    #[test]
    fn constructor_rejects_empty_or_invalid_generation() {
        for generation in [
            "",
            " ",
            "not-a-uuid",
            "550e8400-e29b-41d4-a716-44665544000g",
            "550e8400-e29b-41d4-a716-446655440000\n",
        ] {
            let inner = Arc::new(RecordingSink::default());
            let result = GenerationSink::new(inner.clone(), generation);
            assert!(
                matches!(result, Err(GcsError::Config(_))),
                "accepted {generation:?}"
            );
            assert!(inner.writes().is_empty());
        }
    }

    #[tokio::test]
    async fn accepted_uuid_forms_are_canonicalized() {
        for generation in [
            GENERATION,
            "550E8400-E29B-41D4-A716-446655440000",
            "550e8400e29b41d4a716446655440000",
            "{550e8400-e29b-41d4-a716-446655440000}",
            "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
        ] {
            let inner = Arc::new(RecordingSink::default());
            let sink = GenerationSink::new(inner.clone(), generation).unwrap();
            sink.put_raw("raw/xero/page.json", b"{}", &ObjectMeta::default())
                .await
                .unwrap();
            assert_tagged(&inner.writes()[0].meta, &ObjectMeta::default());
            assert_eq!(
                inner.writes()[0].key,
                format!("v2/{GENERATION}/raw/xero/page.json")
            );
        }
    }

    #[tokio::test]
    async fn regular_write_namespaces_key_and_preserves_body_and_original_metadata() {
        let inner = Arc::new(RecordingSink::default());
        let sink = GenerationSink::new(inner.clone(), GENERATION).unwrap();
        let meta = sample_meta();
        let original = meta.clone();
        let key = "raw/xero/tenant/2.0/invoices/page.json";
        let body = b" {\"Invoices\": []}\n";

        sink.put_raw(key, body, &meta).await.unwrap();

        let writes = inner.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].key, format!("v2/{GENERATION}/{key}"));
        assert_eq!(writes[0].body, body);
        assert_eq!(writes[0].content_type, None);
        assert_tagged(&writes[0].meta, &original);
        assert_eq!(meta, original);
    }

    #[tokio::test]
    async fn main_namespace_uses_natural_xero_path_and_source() {
        let inner = Arc::new(RecordingSink::default());
        let sink = GenerationSink::new_main(inner.clone(), GENERATION).unwrap();

        sink.put_raw(
            "raw/xero/tenant/2.0/payments/page.json",
            b"{}",
            &sample_meta(),
        )
        .await
        .unwrap();

        let write = &inner.writes()[0];
        assert_eq!(
            write.key,
            format!("xero/{GENERATION}/raw/xero/tenant/2.0/payments/page.json")
        );
        assert_eq!(write.meta.get("x-source-system"), Some("xero"));
        assert_eq!(write.meta.get("x-ingestion-generation"), Some(GENERATION));
    }

    #[tokio::test]
    async fn typed_binary_write_preserves_content_type_and_original_metadata() {
        let inner = Arc::new(RecordingSink::default());
        let sink = GenerationSink::new(inner.clone(), GENERATION).unwrap();
        let meta = sample_meta();
        let original = meta.clone();
        let body = b"%PDF-1.7\0\xff\r\n";
        let content_type = "application/pdf; version=1.7";
        let key = "raw/xero/tenant/2.0/accounting_get_getInvoiceAsPdf/document.pdf";

        sink.put_raw_typed(key, body, &meta, content_type)
            .await
            .unwrap();

        let writes = inner.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].key, format!("v2/{GENERATION}/{key}"));
        assert_eq!(writes[0].body, body);
        assert_eq!(writes[0].content_type.as_deref(), Some(content_type));
        assert_tagged(&writes[0].meta, &original);
        assert_eq!(meta, original);
    }

    #[tokio::test]
    async fn local_sink_tags_regular_and_binary_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let sink =
            GenerationSink::new(Arc::new(LocalDirSink::new(dir.path())), GENERATION).unwrap();
        let meta = sample_meta();
        let original = meta.clone();
        let cases: [(&str, &[u8], Option<&str>); 2] = [
            ("raw/xero/page.json", b" {\"Invoices\": []}\n", None),
            (
                "raw/xero/document.pdf",
                b"%PDF-1.7\0\xff",
                Some("application/pdf"),
            ),
        ];
        for (key, body, content_type) in cases {
            match content_type {
                Some(content_type) => sink.put_raw_typed(key, body, &meta, content_type).await,
                None => sink.put_raw(key, body, &meta).await,
            }
            .unwrap();
            let namespaced = format!("v2/{GENERATION}/{key}");
            assert_eq!(
                tokio::fs::read(dir.path().join(&namespaced)).await.unwrap(),
                body
            );
            assert_tagged(&read_sidecar(dir.path(), &namespaced).await, &original);
            assert!(!dir.path().join(key).exists());
            assert!(!dir.path().join(sidecar_key(key)).exists());
            assert_eq!(meta, original);
        }
    }

    #[tokio::test]
    async fn manifest_and_sidecar_are_tagged_without_changing_manifest_body() {
        let dir = tempfile::tempdir().unwrap();
        let inner = Arc::new(RecordingSink::default());
        let recording = GenerationSink::new(inner.clone(), GENERATION).unwrap();
        let local =
            GenerationSink::new(Arc::new(LocalDirSink::new(dir.path())), GENERATION).unwrap();
        let run_id = Uuid::parse_str("aabbccdd-0000-4000-8000-000000000001").unwrap();
        let run_date = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        let manifest = RunManifest {
            run_id: run_id.to_string(),
            started_at: "2026-10-06T00:00:00Z".into(),
            finished_at: "2026-10-06T00:01:00Z".into(),
            mode: "incremental".into(),
            window_days: Some(3),
            entities: vec![],
        };
        let original_meta = ObjectMeta(BTreeMap::from([
            ("x-vendor".into(), "xero".into()),
            ("x-sync-type".into(), "manifest".into()),
            ("x-run-id".into(), manifest.run_id.clone()),
            ("x-synced-at".into(), manifest.finished_at.clone()),
            ("x-run-started-at".into(), manifest.started_at.clone()),
            ("x-http-status".into(), "200".into()),
            ("x-record-count".into(), "0".into()),
            ("x-tenant-id".into(), "run".into()),
            ("x-endpoint".into(), "sync_manifest".into()),
        ]));
        let original_key = manifest_key(run_date, run_id);
        let key = format!("v2/{GENERATION}/{original_key}");
        let body = serde_json::to_vec_pretty(&manifest).unwrap();

        write_manifest(&recording, run_date, run_id, &manifest)
            .await
            .unwrap();
        write_manifest(&local, run_date, run_id, &manifest)
            .await
            .unwrap();

        let writes = inner.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].key, key);
        assert_eq!(writes[0].body, body);
        assert_eq!(writes[0].content_type, None);
        assert_tagged(&writes[0].meta, &original_meta);
        assert_eq!(tokio::fs::read(dir.path().join(&key)).await.unwrap(), body);
        assert_eq!(read_sidecar(dir.path(), &key).await, writes[0].meta);
        assert!(!dir.path().join(&original_key).exists());
        assert!(!dir.path().join(sidecar_key(&original_key)).exists());
    }

    #[tokio::test]
    async fn replay_replaces_old_generation_tags_without_mutating_input() {
        let dir = tempfile::tempdir().unwrap();
        let sink =
            GenerationSink::new(Arc::new(LocalDirSink::new(dir.path())), GENERATION).unwrap();
        let meta = ObjectMeta(
            sample_meta()
                .0
                .into_iter()
                .chain([
                    ("x-source-system".into(), "xero_v1".into()),
                    (
                        "x-ingestion-generation".into(),
                        "00000000-0000-0000-0000-000000000000".into(),
                    ),
                ])
                .collect(),
        );
        let original = meta.clone();
        let key = "raw/xero/replay.json";
        let namespaced = format!("v2/{GENERATION}/{key}");
        let body = b"{\"Invoices\":[]}";

        sink.put_raw(key, body, &meta).await.unwrap();
        let first = tokio::fs::read(dir.path().join(sidecar_key(&namespaced)))
            .await
            .unwrap();
        sink.put_raw(key, body, &meta).await.unwrap();
        let replay = tokio::fs::read(dir.path().join(sidecar_key(&namespaced)))
            .await
            .unwrap();

        assert_eq!(first, replay);
        assert_eq!(
            tokio::fs::read(dir.path().join(&namespaced)).await.unwrap(),
            body
        );
        assert_tagged(&read_sidecar(dir.path(), &namespaced).await, &original);
        assert_eq!(meta, original);
    }

    #[test]
    fn generation_key_namespaces_raw_manifest_and_binary_paths() {
        let sink = GenerationSink::new(Arc::new(RecordingSink::default()), GENERATION).unwrap();
        for key in [
            "raw/xero/tenant/2.0/invoices/page.json",
            "_manifests/xero/2026-10-06/run.json",
            "raw/xero/tenant/2.0/accounting_get_getInvoiceAsPdf/document.pdf",
        ] {
            assert_eq!(
                sink.generation_key(key).unwrap(),
                format!("v2/{GENERATION}/{key}")
            );
        }
    }

    #[test]
    fn generation_key_rejects_unsafe_and_already_namespaced_paths() {
        let sink = GenerationSink::new(Arc::new(RecordingSink::default()), GENERATION).unwrap();
        for key in UNSAFE_KEYS {
            assert!(
                matches!(sink.generation_key(key), Err(GcsError::Config(_))),
                "accepted {key:?}"
            );
        }
    }

    #[tokio::test]
    async fn unsafe_regular_and_typed_writes_never_reach_inner_sink() {
        let inner = Arc::new(RecordingSink::default());
        let sink = GenerationSink::new(inner.clone(), GENERATION).unwrap();
        for key in UNSAFE_KEYS {
            let meta = sample_meta();
            for result in [
                sink.put_raw(key, b"{}", &meta).await,
                sink.put_raw_typed(key, b"%PDF", &meta, "application/pdf")
                    .await,
            ] {
                assert!(
                    matches!(result, Err(GcsError::Config(_))),
                    "accepted {key:?}"
                );
            }
        }
        assert!(inner.writes().is_empty());
    }

    #[tokio::test]
    async fn fresh_generations_do_not_overwrite_legacy_or_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let inner = Arc::new(LocalDirSink::new(dir.path()));
        let meta = sample_meta();
        let key = "raw/xero/tenant/2.0/invoices/page.json";
        inner.put_raw(key, b"legacy", &meta).await.unwrap();
        let legacy_sidecar = tokio::fs::read(dir.path().join(sidecar_key(key)))
            .await
            .unwrap();
        let first = GenerationSink::new(inner.clone(), GENERATION).unwrap();
        let second_generation = "661e8400-e29b-41d4-a716-446655440000";
        let second = GenerationSink::new(inner, second_generation).unwrap();

        first
            .put_raw(key, b"first generation", &meta)
            .await
            .unwrap();
        second
            .put_raw(key, b"second generation", &meta)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(dir.path().join(key)).await.unwrap(),
            b"legacy"
        );
        assert_eq!(
            tokio::fs::read(dir.path().join(sidecar_key(key)))
                .await
                .unwrap(),
            legacy_sidecar
        );
        for (generation, body) in [
            (GENERATION, b"first generation".as_slice()),
            (second_generation, b"second generation".as_slice()),
        ] {
            let namespaced = format!("v2/{generation}/{key}");
            assert_eq!(
                tokio::fs::read(dir.path().join(&namespaced)).await.unwrap(),
                body
            );
            assert_eq!(
                read_sidecar(dir.path(), &namespaced)
                    .await
                    .get("x-ingestion-generation"),
                Some(generation)
            );
        }
    }

    struct FailingSink;

    #[async_trait::async_trait]
    impl RawSink for FailingSink {
        async fn put_raw(&self, _key: &str, _body: &[u8], _meta: &ObjectMeta) -> Result<()> {
            Err(GcsError::Upload("fixture failure".into()))
        }
    }

    #[tokio::test]
    async fn regular_and_typed_writes_propagate_inner_errors() {
        let sink = GenerationSink::new(Arc::new(FailingSink), GENERATION).unwrap();
        let meta = sample_meta();
        let original = meta.clone();
        for result in [
            sink.put_raw("raw/xero/raw.json", b"{}", &meta).await,
            sink.put_raw_typed("raw/xero/document.pdf", b"%PDF", &meta, "application/pdf")
                .await,
        ] {
            assert!(
                matches!(result, Err(GcsError::Upload(message)) if message == "fixture failure")
            );
        }
        assert_eq!(meta, original);
    }
}
