fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(
            &["../../../Model Plane/proto/model_plane/v1/inference.proto"],
            &["../../../Model Plane/proto"],
        )?;
    Ok(())
}
