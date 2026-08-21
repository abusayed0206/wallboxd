fn main() {
    let mut res = winresource::WindowsResource::new();
    // Resource ID "1" is what tray.rs's app_icon() loads via LoadIconW —
    // keep these in sync if either changes. This is also the icon Explorer,
    // Task Manager, and Alt-Tab show for Wallboxd.exe, since Windows treats
    // the resource with the lowest ID as an executable's default icon.
    res.set_icon_with_id("assets/icon.ico", "1");
    res.compile()
        .expect("failed to compile Windows icon resource");
}
