"""Generated protobuf stubs — compiled by scripts/compile_proto.sh.

This package is populated at build time:
    python -m grpc_tools.protoc \\
        -I proto \\
        --python_out=app/grpc_gen \\
        --pyi_out=app/grpc_gen \\
        --grpc_python_out=app/grpc_gen \\
        proto/ai_core.proto

Do NOT import from this package directly in tests; the grpc_servicers
all guard with `try: import ... except ImportError`.
"""
