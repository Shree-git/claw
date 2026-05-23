use clap::Args;
use serde::Serialize;

const OBJECT_FORMAT_VERSION: u8 = 1;

#[derive(Args)]
pub struct VersionArgs {
    /// Output version metadata as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct VersionInfo {
    schema_version: u8,
    action: &'static str,
    name: &'static str,
    version: &'static str,
    package: &'static str,
    git_sha: Option<&'static str>,
    object_format_version: u8,
    sync_protocol_version: &'static str,
    sync_capabilities: Vec<String>,
    build: BuildInfo,
    os: &'static str,
    arch: &'static str,
}

#[derive(Serialize)]
struct BuildInfo {
    date: Option<&'static str>,
    target: String,
    features: Vec<String>,
}

pub fn run(args: VersionArgs) -> anyhow::Result<()> {
    let info = VersionInfo {
        schema_version: 1,
        action: "version",
        name: "claw",
        version: env!("CARGO_PKG_VERSION"),
        package: env!("CARGO_PKG_NAME"),
        git_sha: option_env!("CLAW_GIT_SHA"),
        object_format_version: OBJECT_FORMAT_VERSION,
        sync_protocol_version: claw_sync::protocol::SYNC_PROTOCOL_VERSION,
        sync_capabilities: claw_sync::protocol::server_capabilities(),
        build: BuildInfo {
            date: option_env!("CLAW_BUILD_DATE"),
            target: build_target(),
            features: build_features(),
        },
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("{} {}", info.name, info.version);
    }

    Ok(())
}

fn build_target() -> String {
    option_env!("CLAW_BUILD_TARGET")
        .or(option_env!("TARGET"))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS))
}

fn build_features() -> Vec<String> {
    option_env!("CLAW_BUILD_FEATURES")
        .map(|features| {
            features
                .split(',')
                .map(str::trim)
                .filter(|feature| !feature.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{build_features, build_target};

    #[test]
    fn build_target_has_fallback() {
        assert!(!build_target().is_empty());
    }

    #[test]
    fn build_features_are_available_as_a_list() {
        let _features: Vec<String> = build_features();
    }
}
