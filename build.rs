fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("packaging/windows/mouse-suite.ico");
        res.set("ProductName", "Mouse Suite");
        res.set("FileDescription", "Mouse Suite");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winres icon embed failed: {e}");
        }
    }
}
