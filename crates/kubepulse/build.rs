fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("../../assets/logo.ico")
        .compile()
        .expect("failed to embed the Windows icon");
}
