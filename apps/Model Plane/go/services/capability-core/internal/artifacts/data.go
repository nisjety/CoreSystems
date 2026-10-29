package artifacts

const idemPrefix = "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637"

// catalog is the in-memory seed list of artifact records documenting the
// result-store expectations for large multimodal outputs. Every entry is
// scoped to the "triodelab" organization.
var catalog = Catalog{
	Artifacts: []Artifact{
		{
			ID:             "artifact.image.sample-01",
			IdempotencyKey: idemPrefix + ":artifact.image.sample-01",
			OrgID:          "triodelab",
			Kind:           KindImage,
			ModalityID:     "modality.images",
			ProducerRunID:  "run.model-plane.bootstrap",
			MimeType:       "image/png",
			SizeBytes:      2_457_600,
			StorageURI:     "s3://triodelab-artifacts/images/sample-01.png",
			RetentionDays:  30,
			CreatedAt:      "2025-01-01T00:30:00Z",
			Description:    "Reference image artifact proving offload path for images modality.",
		},
		{
			ID:             "artifact.audio.sample-01",
			IdempotencyKey: idemPrefix + ":artifact.audio.sample-01",
			OrgID:          "triodelab",
			Kind:           KindAudio,
			ModalityID:     "modality.speech",
			ProducerRunID:  "run.model-plane.bootstrap",
			MimeType:       "audio/wav",
			SizeBytes:      8_192_000,
			StorageURI:     "s3://triodelab-artifacts/audio/sample-01.wav",
			RetentionDays:  14,
			CreatedAt:      "2025-01-01T00:35:00Z",
			Description:    "Reference audio artifact for TTS and streaming transcription outputs.",
		},
		{
			ID:             "artifact.video.sample-01",
			IdempotencyKey: idemPrefix + ":artifact.video.sample-01",
			OrgID:          "triodelab",
			Kind:           KindVideo,
			ModalityID:     "modality.video",
			ProducerRunID:  "run.model-plane.bootstrap",
			MimeType:       "video/mp4",
			SizeBytes:      104_857_600,
			StorageURI:     "s3://triodelab-artifacts/video/sample-01.mp4",
			RetentionDays:  7,
			CreatedAt:      "2025-01-01T00:40:00Z",
			Description:    "Reference video artifact for long-running video synthesis outputs.",
		},
		{
			ID:             "artifact.document.sample-01",
			IdempotencyKey: idemPrefix + ":artifact.document.sample-01",
			OrgID:          "triodelab",
			Kind:           KindDocument,
			ModalityID:     "modality.documents",
			ProducerRunID:  "run.model-plane.bootstrap",
			MimeType:       "application/pdf",
			SizeBytes:      1_048_576,
			StorageURI:     "s3://triodelab-artifacts/documents/sample-01.pdf",
			RetentionDays:  90,
			CreatedAt:      "2025-01-01T00:45:00Z",
			Description:    "Reference document artifact for parsed document extraction outputs.",
		},
	},
}
