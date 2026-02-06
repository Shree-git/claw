fn main() -> Result<(), Box<dyn std::error::Error>> {
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
