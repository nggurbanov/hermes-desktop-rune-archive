fn main() {
    println!("cargo:rerun-if-changed=vendor/sonic/sonic.c");
    println!("cargo:rerun-if-changed=vendor/sonic/sonic.h");
    cc::Build::new()
        .file("vendor/sonic/sonic.c")
        .include("vendor/sonic")
        .compile("hermes_sonic");
}
