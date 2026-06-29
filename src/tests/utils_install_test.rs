use crate::utils::install::*;

#[test]
fn detect_returns_some_variant() {
    let method = InstallMethod::detect();
    // On CI or dev machines this should return Source (we're in a cargo project)
    // Just verify it doesn't panic and returns a valid variant
    let _ = method.description();
}

#[test]
fn source_description() {
    let m = InstallMethod::Source(std::path::PathBuf::from("/tmp"));
    assert_eq!(m.description(), "source build");
}

#[test]
fn cargo_install_description() {
    assert_eq!(InstallMethod::CargoInstall.description(), "cargo install");
}

#[test]
fn prebuilt_description() {
    assert_eq!(
        InstallMethod::PrebuiltBinary.description(),
        "pre-built binary"
    );
}

#[test]
fn platform_suffix_is_some_on_supported() {
    // On any standard dev machine this should return Some
    if matches!(
        (std::env::consts::OS, std::env::consts::ARCH),
        ("macos", "aarch64")
            | ("macos", "x86_64")
            | ("linux", "x86_64")
            | ("linux", "aarch64")
            | ("windows", "x86_64")
    ) {
        assert!(platform_suffix().is_some());
    }
}

#[test]
fn binary_name_matches_platform() {
    let name = binary_name();
    if std::env::consts::OS == "windows" {
        assert_eq!(name, "opencrabs.exe");
    } else {
        assert_eq!(name, "opencrabs");
    }
}
