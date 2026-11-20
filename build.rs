//! Writes into the Windows program what it is: its name, its version, who makes it and where its code is, and a
//! manifest that says which Windows it is made for and that it starts with the rights of whoever starts it. A file
//! without any of this is anonymous to Windows and to antivirus programs (20 Nov 2026: Microsoft Defender took the
//! unsigned, unnamed program for a trojan on a tester's PC). The version comes from Cargo.toml.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="FPStick.Recorder" version="{version}.0" processorArchitecture="amd64"/>
  <description>FPStick Recorder: records a match of Overwatch with Intel PresentMon for a run page on fpstick.com</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
</assembly>
"#
    );
    let mut res = winresource::WindowsResource::new();
    res.set("ProductName", "FPStick Recorder")
        .set("FileDescription", "FPStick Recorder")
        .set("CompanyName", "The FPStick contributors")
        .set("LegalCopyright", "The FPStick contributors. MIT or Apache-2.0.")
        .set("OriginalFilename", "FPStick Recorder.exe")
        .set("InternalName", "recorder")
        .set("Comments", "Open source: https://github.com/Gen3tics/fpstick-recorder  Site: https://fpstick.com")
        .set_manifest(&manifest);
    if let Err(e) = res.compile_for("recorder") {
        panic!("The program's name and version could not be written into it: {}", e);
    }
}
