fn main() {
    let mut res = winresource::WindowsResource::new();
    // Resource ID "1" is what tray.rs's app_icon() loads via LoadIconW —
    // keep these in sync if either changes. This is also the icon Explorer,
    // Task Manager, and Alt-Tab show for Wallboxd.exe, since Windows treats
    // the resource with the lowest ID as an executable's default icon.
    res.set_icon_with_id("assets/icon.ico", "1");
    // Opts into themed (v6) common controls — see assets/app.manifest's
    // comment for why this matters for how every dialog control looks.
    res.set_manifest_file("assets/app.manifest");
    res.compile().expect("failed to compile Windows resources");

    // Short commit hash for the settings dialog's version/build label, read
    // via env!("WALLBOXD_GIT_HASH") in settings_dialog.rs. Falls back to
    // "dev" for builds outside a git checkout (e.g. a source tarball)
    // instead of failing the build over a cosmetic label.
    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dev".to_string());
    println!("cargo:rustc-env=WALLBOXD_GIT_HASH={git_hash}");
    // Re-run only when HEAD actually moves, not on every build.
    println!("cargo:rerun-if-changed=.git/HEAD");
}
