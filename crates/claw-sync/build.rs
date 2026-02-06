fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto";
    let protos = &[
        "claw/common.proto",
        "claw/objects.proto",
        "claw/sync.proto",
        "claw/intent.proto",
        "claw/change.proto",
        "claw/capsule.proto",
        "claw/workstream.proto",
        "claw/event.proto",
    ];

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, &[proto_root])?;

    Ok(())
}
