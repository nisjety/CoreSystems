use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Includes for proto compilation:
    //   - "../../proto" → our service-defined messages
    //   - well-known types path → google/protobuf/{struct,timestamp}.proto
    //
    // Well-known types are bundled with the protoc compiler. Search common
    // install paths so the build works on:
    //   - macOS Homebrew (/opt/homebrew/include)
    //   - Linux distros with libprotobuf-dev (/usr/include)
    //   - Docker images that set PROTOC_INCLUDE explicitly
    let mut includes: Vec<PathBuf> = vec![
        PathBuf::from("../../proto"),
        PathBuf::from("../../../Model Plane/proto"),
    ];

    if let Ok(p) = std::env::var("PROTOC_INCLUDE") {
        includes.push(PathBuf::from(p));
    }
    for candidate in [
        "/usr/include",
        "/opt/homebrew/include",
        "/usr/local/include",
    ] {
        let path = PathBuf::from(candidate).join("google/protobuf/timestamp.proto");
        if path.exists() {
            includes.push(PathBuf::from(candidate));
            break;
        }
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "../../proto/retrieval_v2.proto",
                "../../proto/documents_v2.proto",
                "../../proto/knowledge_v2.proto",
                "../../../Model Plane/proto/model_plane/v1/inference.proto",
            ],
            &includes,
        )?;
    Ok(())
}
