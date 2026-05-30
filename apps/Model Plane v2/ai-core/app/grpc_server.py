"""gRPC server for ai-core v2.

Usage
-----
Called from main.py lifespan:

    from app.grpc_server import create_grpc_server
    grpc_server = await create_grpc_server(port=settings.grpc_port)

The server is started concurrently with uvicorn. It shuts down cleanly on
lifespan exit.

Proto compilation
-----------------
Run once (or in Dockerfile build stage):

    python -m grpc_tools.protoc \\
        -I proto \\
        --python_out=app/grpc_gen \\
        --pyi_out=app/grpc_gen \\
        --grpc_python_out=app/grpc_gen \\
        proto/ai_core.proto
"""

from __future__ import annotations

import logging

from app.config import get_settings

logger = logging.getLogger(__name__)

_server_instance = None


def _channel_options() -> list[tuple[str, int]]:
    """Build gRPC channel options from settings."""
    s = get_settings()
    max_msg = getattr(s, "grpc_max_message_length", 52_428_800)
    keepalive_ms = getattr(s, "grpc_keepalive_time_ms", 30_000)
    keepalive_timeout_ms = getattr(s, "grpc_keepalive_timeout_ms", 5_000)
    return [
        ("grpc.max_send_message_length", max_msg),
        ("grpc.max_receive_message_length", max_msg),
        ("grpc.keepalive_time_ms", keepalive_ms),
        ("grpc.keepalive_timeout_ms", keepalive_timeout_ms),
        ("grpc.keepalive_permit_without_calls", 1),
        ("grpc.http2.max_pings_without_data", 0),
    ]


async def create_grpc_server(port: int = 50051):
    """Start the gRPC server on *port*.

    Returns the server instance. Returns ``None`` if grpc is not installed or
    compiled stubs are absent (allows the rest of the service to boot cleanly
    in environments without the gRPC toolchain).
    """
    global _server_instance

    # The generated stubs use top-level `import ai_core_pb2` which requires
    # the grpc_gen directory to be on sys.path.
    import os
    import sys
    _grpc_gen_dir = os.path.join(os.path.dirname(__file__), "grpc_gen")
    if _grpc_gen_dir not in sys.path:
        sys.path.insert(0, _grpc_gen_dir)

    try:
        import grpc
        import grpc.aio
        from app.grpc_gen import ai_core_pb2_grpc  # type: ignore[import]
    except ImportError as exc:
        logger.warning("grpc_unavailable reason=%s — gRPC server not started", exc)
        return None

    settings = get_settings()

    from app.grpc_servicers.chat_servicer import ChatServicer
    from app.grpc_servicers.speech_servicer import SpeechServicer
    from app.grpc_servicers.document_servicer import DocumentServicer
    from app.grpc_servicers.image_servicer import ImageServicer
    from app.grpc_servicers.translation_servicer import TranslationServicer

    server = grpc.aio.server(options=_channel_options())

    # Register all proto-defined services
    ai_core_pb2_grpc.add_ChatServiceServicer_to_server(ChatServicer(), server)
    ai_core_pb2_grpc.add_SpeechServiceServicer_to_server(SpeechServicer(), server)
    ai_core_pb2_grpc.add_DocumentServiceServicer_to_server(DocumentServicer(), server)
    ai_core_pb2_grpc.add_ImageServiceServicer_to_server(ImageServicer(), server)
    ai_core_pb2_grpc.add_TranslationServiceServicer_to_server(TranslationServicer(), server)

    # Health checking (standard gRPC health protocol)
    try:
        from grpc_health.v1 import health as grpc_health
        from grpc_health.v1 import health_pb2, health_pb2_grpc

        health_servicer = grpc_health.HealthServicer()
        health_pb2_grpc.add_HealthServicer_to_server(health_servicer, server)
        # Mark all services as SERVING
        for svc_name in [
            "ai_core.v2.ChatService",
            "ai_core.v2.SpeechService",
            "ai_core.v2.DocumentService",
            "ai_core.v2.ImageService",
            "ai_core.v2.TranslationService",
            "",  # overall server health
        ]:
            health_servicer.set(svc_name, health_pb2.HealthCheckResponse.SERVING)
        logger.info("grpc_health_check registered")
    except ImportError:
        logger.warning("grpc_health package not installed — health check unavailable")

    # Server reflection (for grpcurl / grpc_cli discovery)
    enable_reflection = getattr(settings, "grpc_enable_reflection", True)
    if enable_reflection:
        try:
            from grpc_reflection.v1alpha import reflection as grpc_reflection

            service_names = [
                "ai_core.v2.ChatService",
                "ai_core.v2.SpeechService",
                "ai_core.v2.DocumentService",
                "ai_core.v2.ImageService",
                "ai_core.v2.TranslationService",
            ]
            grpc_reflection.enable_server_reflection(service_names, server)
            logger.info("grpc_reflection enabled services=%d", len(service_names))
        except ImportError:
            logger.warning("grpc_reflection package not installed — reflection unavailable")

    listen_addr = f"[::]:{port}"
    server.add_insecure_port(listen_addr)

    try:
        await server.start()
        logger.info("grpc_server_started port=%d services=5 health=true reflection=%s",
                     port, enable_reflection)
    except Exception as exc:
        logger.error("grpc_server_start_failed port=%d error=%s", port, exc)
        return None

    _server_instance = server
    return server


async def stop_grpc_server(grace: float = 5.0) -> None:
    """Gracefully stop the global gRPC server instance."""
    global _server_instance

    if _server_instance is None:
        return

    try:
        await _server_instance.stop(grace)
        logger.info("grpc_server_stopped")
    except Exception as exc:
        logger.warning("grpc_server_stop_error error=%s", exc)
    finally:
        _server_instance = None
