fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Prefer the user's protoc if they set PROTOC, otherwise fall back to a
    // vendored protoc binary so contributors don't need a system install.
    if std::env::var_os("PROTOC").is_none() {
        let protoc = protoc_bin_vendored::protoc_bin_path()?;
        std::env::set_var("PROTOC", protoc);
    }
    println!("cargo:rerun-if-env-changed=PROTOC");

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

    for proto in protos {
        println!("cargo:rerun-if-changed={proto_root}/{proto}");
    }

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(protos, &[proto_root])?;

    Ok(())
}
