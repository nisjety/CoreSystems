//! Opening the person's browser.

use std::process::{Command, Stdio};

use url::Url;

use crate::login::Browser;

/// The operating system's default browser. The address is passed as one argument, never through a shell.
pub struct SystemBrowser;

impl Browser for SystemBrowser {
    fn open(&self, address: &str) -> bool {
        let web = Url::parse(address).is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
        if !web {
            return false;
        }
        let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("open", vec![address])
        } else if cfg!(target_os = "windows") {
            ("rundll32", vec!["url.dll,FileProtocolHandler", address])
        } else {
            ("xdg-open", vec![address])
        };
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_addresses_are_ever_handed_to_the_system() {
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "-a Calculator",
            "",
            "ftp://x/y",
        ] {
            assert!(!SystemBrowser.open(refused), "{refused:?}");
        }
    }
}
