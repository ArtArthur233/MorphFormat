fn main() {
    // Embed the icon and version info in the Windows exe.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Morph-Format");
        res.set("FileDescription", "Morph-Format");
        res.compile().expect("failed to embed Windows resources");
    }
}
