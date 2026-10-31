use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let proto_root = manifest_dir.join("../../contract/proto");

    let protos = [
        proto_root.join("darkfactory/transcript/v1/transcript.proto"),
        proto_root.join("darkfactory/agent/v1/agent.proto"),
        proto_root.join("darkfactory/gateway/v1/gateway.proto"),
    ];

    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
    println!("cargo:rerun-if-env-changed=PROTOC");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let descriptor_path = out_dir.join("proto_descriptor.bin");

    let mut config = prost_build::Config::new();
    config.bytes(["."]);

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .build_transport(false)
        .compile_well_known_types(true)
        .extern_path(".google.protobuf", "::pbjson_types")
        // Implemented in `lib.rs` without the password.
        .skip_debug([".darkfactory.gateway.v1.ForgeCredential"])
        .file_descriptor_set_path(&descriptor_path)
        .compile_with_config(config, &protos, &[proto_root])?;

    let descriptor_set = std::fs::read(&descriptor_path)?;
    pbjson_build::Builder::new()
        .register_descriptors(&descriptor_set)?
        // Forge credentials, and the messages carrying them, never get a JSON form.
        .exclude([
            ".darkfactory.gateway.v1.ForgeCredential",
            ".darkfactory.gateway.v1.Snapshot",
            ".darkfactory.gateway.v1.PolicyUpdate",
        ])
        .build(&[".darkfactory"])?;

    Ok(())
}
