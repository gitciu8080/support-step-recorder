fn main() {
    println!("cargo:rerun-if-changed=src/input/a11y.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        pkg_config::Config::new()
            .probe("gobject-2.0")
            .expect("Install libglib2.0-dev");
        let lib = pkg_config::Config::new()
            .probe("atspi-2")
            .expect("Install libatspi2.0-dev for committed field text capture");
        let mut build = cc::Build::new();
        build.file("src/input/a11y.c");
        for path in lib.include_paths {
            build.include(path);
        }
        build.compile("ssr_a11y");
    }
}
