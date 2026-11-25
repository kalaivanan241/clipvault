//! Open ClipVault at login, through a LaunchAgent in `~/Library/LaunchAgents`

use anyhow::Result;

#[cfg(target_os = "macos")]
fn agent() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    // The bundle id lets System Settings → Login Items show it as ClipVault
    let agent = auto_launch::AutoLaunchBuilder::new()
        .set_app_name("ClipVault")
        .set_app_path(&exe.to_string_lossy())
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .set_bundle_identifiers(&["local.clipvault"])
        .build()?;
    Ok(agent)
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    agent().is_ok_and(|a| a.is_enabled().unwrap_or(false))
}

#[cfg(target_os = "macos")]
pub fn set_enabled(on: bool) -> Result<()> {
    let agent = agent()?;
    if on {
        agent.enable()?;
    } else {
        agent.disable()?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn set_enabled(_on: bool) -> Result<()> {
    Err(anyhow::anyhow!("open at login is only supported on macOS"))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn toggles_a_launch_agent_for_this_binary() {
        // A throwaway home so the real ~/Library/LaunchAgents is untouched
        let home = std::env::temp_dir().join(format!("clipvault-login-{}", std::process::id()));
        std::fs::create_dir_all(home.join("Library")).unwrap();
        std::env::set_var("HOME", &home);
        let plist = home.join("Library/LaunchAgents/ClipVault.plist");

        set_enabled(true).unwrap();
        assert!(is_enabled());
        let body = std::fs::read_to_string(&plist).unwrap();
        let exe = std::env::current_exe().unwrap();
        assert!(body.contains(&*exe.to_string_lossy()));
        assert!(body.contains("<key>RunAtLoad</key>"));

        set_enabled(false).unwrap();
        assert!(!is_enabled());
        let _ = std::fs::remove_dir_all(&home);
    }
}
