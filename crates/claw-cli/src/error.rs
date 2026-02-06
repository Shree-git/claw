// CLI errors are handled via anyhow at the top level.
// This module provides helpers for formatting errors.

pub fn format_error(err: &anyhow::Error) -> String {
    let mut msg = format!("error: {err}");
    for cause in err.chain().skip(1) {
        msg.push_str(&format!("\n  caused by: {cause}"));
    }
    msg
}
