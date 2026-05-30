"""Tests for the Model Plane v2 parity catalogue."""

from __future__ import annotations

from provider_research.parity import (
    MODEL_PLANE_V2_PARITY,
    ParityStatus,
    missing_capabilities,
    parity_matrix,
)


def test_parity_catalog_has_unique_ids() -> None:
    ids = [capability.capability_id for capability in MODEL_PLANE_V2_PARITY]
    assert len(ids) == len(set(ids))


def test_parity_catalog_covers_required_v2_ai_surfaces() -> None:
    ids = {capability.capability_id for capability in MODEL_PLANE_V2_PARITY}
    assert {
        "ai.embeddings",
        "ai.images",
        "ai.speech",
        "ai.translation",
        "ai.document_intelligence",
        "ai.realtime",
        "ai.video",
    }.issubset(ids)


def test_embedding_parity_tracks_current_model_plane_endpoint() -> None:
    embedding = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.embeddings"
    )
    assert embedding.status is ParityStatus.yes
    assert "InferenceCore.CreateEmbedding" in embedding.current_surface
    assert "Azure OpenAI" in embedding.next_step


def test_translation_parity_tracks_current_model_plane_endpoint() -> None:
    translation = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.translation"
    )
    assert translation.status is ParityStatus.partial
    assert "InferenceCore.TranslateText" in translation.current_surface
    assert "Azure Translator" in translation.next_step


def test_image_parity_tracks_current_model_plane_endpoint() -> None:
    images = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.images"
    )
    assert images.status is ParityStatus.partial
    assert "InferenceCore.GenerateImage" in images.current_surface
    assert "Azure Document Intelligence" in images.next_step


def test_document_intelligence_parity_tracks_current_model_plane_endpoint() -> None:
    document_intelligence = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.document_intelligence"
    )
    assert document_intelligence.status is ParityStatus.partial
    assert "InferenceCore.AnalyzeDocument" in document_intelligence.current_surface
    assert "invoice/receipt" in document_intelligence.next_step


def test_language_analytics_parity_tracks_current_model_plane_endpoint() -> None:
    language = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.language_analytics"
    )
    assert language.status is ParityStatus.partial
    assert "InferenceCore.AnalyzeLanguage" in language.current_surface
    assert "Azure AI Language" in language.next_step


def test_realtime_parity_tracks_current_model_plane_endpoint() -> None:
    realtime = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.realtime"
    )
    assert realtime.status is ParityStatus.partial
    assert "InferenceCore.CreateRealtimeSession" in realtime.current_surface
    assert "OpenAI realtime" in realtime.next_step


def test_video_parity_tracks_current_model_plane_endpoint() -> None:
    video = next(
        capability
        for capability in MODEL_PLANE_V2_PARITY
        if capability.capability_id == "ai.video"
    )
    assert video.status is ParityStatus.partial
    assert "InferenceCore.CreateVideoGenerationJob" in video.current_surface
    assert "Azure OpenAI Sora" in video.next_step


def test_parity_matrix_filters_by_status() -> None:
    missing = parity_matrix(ParityStatus.no)
    assert missing
    assert all(entry["status"] == "no" for entry in missing)


def test_missing_capabilities_excludes_completed_items() -> None:
    assert all(capability.status != ParityStatus.yes for capability in missing_capabilities())
