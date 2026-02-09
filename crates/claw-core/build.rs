fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Prefer the user's protoc if they set PROTOC, otherwise fall back to a
    // vendored protoc binary so contributors don't need a system install.
    if std::env::var_os("PROTOC").is_none() {
        let protoc = protoc_bin_vendored::protoc_bin_path()?;
        std::env::set_var("PROTOC", protoc);
    }
    println!("cargo:rerun-if-env-changed=PROTOC");

    let proto_root = "../../proto";

    prost_build::Config::new()
        .out_dir("src/generated")
        .compile_protos(
            &[
                &format!("{proto_root}/claw/common.proto"),
                &format!("{proto_root}/claw/objects.proto"),
            ],
            &[proto_root],
        )?;

    println!("cargo:rerun-if-changed={proto_root}/claw/common.proto");
    println!("cargo:rerun-if-changed={proto_root}/claw/objects.proto");
    Ok(())
}
