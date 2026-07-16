fn main() {
    // Embed the UAC manifest only when natively compiling ON Windows.
    // Cross-compilation (e.g. from Linux) skips this; the manifest is embedded
    // by the Windows linker during a native build on the target machine.
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_manifest_file("src/main.manifest");
        res.compile().expect("Failed to embed Windows manifest");
    }

    tauri_build::build()
}
