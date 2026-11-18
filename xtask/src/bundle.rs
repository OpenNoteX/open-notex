//! Package the app as a macOS bundle, a disk image, a zip and checksums.
//!
//! A released artifact has to be the same bytes the tests run, so this module is
//! a plain file operation plus the tools macOS already has: `codesign` signs,
//! `hdiutil` makes the disk image, `ditto` makes the zip and `shasum` prints the
//! hashes. Nothing here is a build system; `cargo xtask bundle` is one command
//! that a person and CI both run.
//!
//! The release is Apple Silicon only, so a binary built for anything else is
//! refused here rather than shipped and failing to launch.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The bundle's identity. Changing it resets every permission macOS has granted
/// the app, so it is fixed before the first release.
pub const BUNDLE_ID: &str = "com.opennote.opennotex";
pub const APP_NAME: &str = "openNoteX";
pub const EXE_NAME: &str = "open-notex";
/// The oldest macOS the bundle claims to support.
pub const MIN_MACOS: &str = "13.0";
/// The only architecture this release ships.
pub const ARCH: &str = "arm64";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Arm64,
    X86_64,
    /// A universal ("fat") binary, which this release does not ship.
    Fat,
    /// Not a Mach-O, or a Mach-O this code does not know.
    Unknown,
}

/// Read the architecture from a Mach-O header. Pure, so the guard can be tested
/// without a compiler for the other architecture.
pub fn binary_kind(bytes: &[u8]) -> Binary {
    let Some(header) = bytes.get(..8) else {
        return Binary::Unknown;
    };
    // Both the magic and `cputype` are little-endian on the only machines this
    // builds on, so read the magic the same way as the fields below it.
    let magic = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    match magic {
        // 64-bit and 32-bit Mach-O: `cputype` is the next four bytes.
        0xFEED_FACF | 0xFEED_FACE => {
            let cputype = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            match cputype {
                0x0100_000C => Binary::Arm64,
                0x0100_0007 => Binary::X86_64,
                _ => Binary::Unknown,
            }
        }
        0xBEBA_FECA | 0xBEBA_FECF => Binary::Fat,
        _ => Binary::Unknown,
    }
}

/// Refuse a binary that is not arm64, with the command that fixes it. The
/// message is the whole value of the check: `cargo build` on an Intel Mac, or on
/// a runner that changed architecture, is otherwise a `.dmg` that dies on launch.
pub fn check_arch(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    match binary_kind(&bytes) {
        Binary::Arm64 => Ok(()),
        Binary::X86_64 => Err(format!(
            "{} is x86_64. This release is Apple Silicon only: build it with \
             `cargo build --release` on an arm64 Mac.",
            path.display()
        )),
        Binary::Fat => Err(format!(
            "{} is a universal binary. This release is Apple Silicon only: bundle the \
             arm64 slice, not a fat binary.",
            path.display()
        )),
        Binary::Unknown => Err(format!(
            "{} is not a Mach-O executable, so it cannot be bundled.",
            path.display()
        )),
    }
}

/// How the bundle is signed. `Identity` is a Developer ID from an Apple
/// Developer account; `AdHoc` is what a release without one gets, and it is the
/// minimum on Apple Silicon, which refuses an unsigned binary outright.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sign {
    AdHoc,
    Identity(String),
    /// Tests assemble a layout without asking `codesign` about a stub binary.
    #[cfg(test)]
    None,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub bin: PathBuf,
    pub out: PathBuf,
    /// `CFBundleShortVersionString`, from the workspace version.
    pub version: String,
    /// `CFBundleVersion`, a build number that only has to increase.
    pub build: String,
    pub icon: Option<PathBuf>,
    pub sign: Sign,
    pub dmg: bool,
    pub zip: bool,
}

impl Options {
    /// A bundle in `out`, ad-hoc signed, with no archives.
    pub fn new(bin: impl Into<PathBuf>, out: impl Into<PathBuf>, version: &str) -> Self {
        Self {
            bin: bin.into(),
            out: out.into(),
            version: version.to_owned(),
            build: "1".to_owned(),
            icon: None,
            sign: Sign::AdHoc,
            dmg: false,
            zip: false,
        }
    }
}

/// `openNoteX-0.1.0-arm64`, the stem every artifact shares.
pub fn artifact_stem(version: &str) -> String {
    format!("{APP_NAME}-{version}-{ARCH}")
}

/// The bundle's `Info.plist`. Values are escaped: a plist is XML, and a build
/// number or copyright that contains `&` must not make it unreadable.
pub fn info_plist(version: &str, build: &str, copyright: &str) -> String {
    let entries: [(&str, &str); 12] = [
        ("CFBundleName", APP_NAME),
        ("CFBundleDisplayName", APP_NAME),
        ("CFBundleIdentifier", BUNDLE_ID),
        ("CFBundleExecutable", EXE_NAME),
        ("CFBundleIconFile", "icon"),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleSignature", "????"),
        ("CFBundleShortVersionString", version),
        ("CFBundleVersion", build),
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("LSMinimumSystemVersion", MIN_MACOS),
        // The Dock and Finder treat this as a productivity app, not a terminal.
        (
            "LSApplicationCategoryType",
            "public.app-category.productivity",
        ),
    ];

    let mut out = String::with_capacity(1024);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
    );
    out.push_str("<plist version=\"1.0\">\n<dict>\n");
    for (key, value) in entries {
        out.push_str(&format!(
            "\t<key>{}</key>\n\t<string>{}</string>\n",
            escape(key),
            escape(value)
        ));
    }
    out.push_str("\t<key>NSHighResolutionCapable</key>\n\t<true/>\n");
    out.push_str(&format!(
        "\t<key>NSHumanReadableCopyright</key>\n\t<string>{}</string>\n",
        escape(copyright)
    ));
    out.push_str("</dict>\n</plist>\n");
    out
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Write `OpenNoteX.app` into `out` and sign it. Returns the bundle path.
pub fn assemble(options: &Options) -> Result<PathBuf, String> {
    if !options.bin.is_file() {
        return Err(format!(
            "no executable at {}: build it first",
            options.bin.display()
        ));
    }
    check_arch(&options.bin)?;

    let app = options.out.join(format!("{APP_NAME}.app"));
    if app.exists() {
        std::fs::remove_dir_all(&app).map_err(|error| format!("{}: {error}", app.display()))?;
    }
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&macos).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&resources).map_err(|error| error.to_string())?;

    let exe = macos.join(EXE_NAME);
    std::fs::copy(&options.bin, &exe).map_err(|error| error.to_string())?;
    set_executable(&exe)?;

    std::fs::write(
        app.join("Contents/Info.plist"),
        info_plist(
            &options.version,
            &options.build,
            &format!("{APP_NAME} contributors. MIT licensed."),
        ),
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(app.join("Contents/PkgInfo"), "APPL????").map_err(|error| error.to_string())?;

    if let Some(icon) = &options.icon {
        if !icon.is_file() {
            return Err(format!(
                "no icon at {}: run `scripts/make-icon.sh` to rebuild it from assets/icon.svg",
                icon.display()
            ));
        }
        std::fs::copy(icon, resources.join("icon.icns")).map_err(|error| error.to_string())?;
    }

    sign(&app, &options.sign)?;
    Ok(app)
}

fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

fn sign(app: &Path, sign: &Sign) -> Result<(), String> {
    match sign {
        #[cfg(test)]
        Sign::None => Ok(()),
        Sign::AdHoc => {
            // No `--options runtime` and no `--timestamp`: both need an identity
            // to mean anything, and an ad-hoc signature has neither.
            tool(
                "codesign",
                &["--force", "--sign", "-", &app.display().to_string()],
                None,
            )?;
            tool(
                "codesign",
                &["--verify", "--deep", "--strict", &app.display().to_string()],
                None,
            )?;
            Ok(())
        }
        Sign::Identity(identity) => {
            tool(
                "codesign",
                &[
                    "--force",
                    "--sign",
                    identity,
                    "--options",
                    "runtime",
                    "--timestamp",
                    &app.display().to_string(),
                ],
                None,
            )?;
            tool(
                "codesign",
                &["--verify", "--deep", "--strict", &app.display().to_string()],
                None,
            )?;
            Ok(())
        }
    }
}

/// A compressed disk image with the bundle and a link to `/Applications`, which
/// is what a person drags onto. Staged in `out` and removed afterwards.
fn disk_image(app: &Path, out: &Path, stem: &str) -> Result<PathBuf, String> {
    let stage = out.join(".dmg-stage");
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&stage).map_err(|error| error.to_string())?;

    let staged_app = stage.join(format!("{APP_NAME}.app"));
    tool(
        "ditto",
        &[
            &app.display().to_string(),
            &staged_app.display().to_string(),
        ],
        None,
    )?;
    std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
        .map_err(|error| error.to_string())?;

    let image = out.join(format!("{stem}.dmg"));
    let _ = std::fs::remove_file(&image);
    tool(
        "hdiutil",
        &[
            "create",
            "-quiet",
            "-volname",
            APP_NAME,
            "-srcfolder",
            &stage.display().to_string(),
            "-format",
            "UDZO",
            "-ov",
            &image.display().to_string(),
        ],
        None,
    )?;

    std::fs::remove_dir_all(&stage).map_err(|error| error.to_string())?;
    Ok(image)
}

/// A zip for updates: a disk image cannot be replaced in place, a zip can be
/// unpacked beside the running bundle. `ditto` keeps the bundle's metadata.
fn archive(app: &Path, out: &Path, stem: &str) -> Result<PathBuf, String> {
    let file = out.join(format!("{stem}.zip"));
    let _ = std::fs::remove_file(&file);
    tool(
        "ditto",
        &[
            "-c",
            "-k",
            "--keepParent",
            &app.display().to_string(),
            &file.display().to_string(),
        ],
        None,
    )?;
    Ok(file)
}

/// `shasum` lines for every artifact, so a download can be checked before it is
/// opened. Written next to the artifacts, not into the image.
fn checksums(files: &[PathBuf], out: &Path) -> Result<PathBuf, String> {
    let mut lines = String::new();
    for file in files {
        // Bare file names: `shasum -c SHA256SUMS.txt` has to work in the folder
        // the downloads landed in, not only in the build tree.
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{} has no file name", file.display()))?;
        let stdout = tool("shasum", &["-a", "256", &name], Some(out))?;
        lines.push_str(stdout.trim());
        lines.push('\n');
    }
    let path = out.join("SHA256SUMS.txt");
    std::fs::write(&path, lines).map_err(|error| error.to_string())?;
    Ok(path)
}

fn tool(tool: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new(tool);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .output()
        .map_err(|error| format!("could not run {tool}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{tool} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Launch the bundle the way a person does — from inside `OpenNoteX.app`, with a
/// home of its own — and wait for Rune to say the first window is up. This is the
/// only check that proves the packaged app runs: a copied binary and a wrong
/// `Info.plist` both pass a file listing.
///
/// A window appears for a moment, so it is opt-in (`--check`), not part of
/// packaging. `HOME` is a temp directory, so the check never touches a real
/// vault, database or recovery folder.
fn check(app: &Path, version: &str) -> Result<(), String> {
    let exe = app.join("Contents/MacOS").join(EXE_NAME);

    let reported = Command::new(&exe)
        .arg("--version")
        .output()
        .map_err(|error| format!("{}: {error}", exe.display()))?;
    let line = String::from_utf8_lossy(&reported.stdout).trim().to_owned();
    if !line.starts_with(&format!("{APP_NAME} {version} ")) || !reported.status.success() {
        return Err(format!(
            "the bundled binary reports {line:?}, not {APP_NAME} {version}"
        ));
    }

    let home = tempfile::tempdir().map_err(|error| error.to_string())?;
    let vault = home.path().join("vault");
    let ready = home.path().join("ready");
    std::fs::create_dir_all(&vault).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&ready).map_err(|error| error.to_string())?;

    let mut child = Command::new(&exe)
        .arg("--vault")
        .arg(&vault)
        .env("HOME", home.path())
        .env("RUNE_READY_DIR", &ready)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("could not launch {}: {error}", exe.display()))?;

    let receipt = wait_for_receipt(&ready, Duration::from_secs(20));
    let _ = child.kill();
    let _ = child.wait();
    let receipt = receipt?;

    let body = std::fs::read_to_string(&receipt).map_err(|error| error.to_string())?;
    if !body.contains(&format!("\"bundle_id\":\"{BUNDLE_ID}\"")) {
        return Err(format!(
            "the receipt at {} does not name {BUNDLE_ID}: {body}",
            receipt.display()
        ));
    }
    Ok(())
}

/// The ready receipt Rune writes once the first window is up. Its name is the
/// bundle id and the pid, which is also how the installer finds the process.
fn wait_for_receipt(dir: &Path, timeout: Duration) -> Result<PathBuf, String> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!("{BUNDLE_ID}.")) {
                    return Ok(entry.path());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "the app did not report a ready window within {}s",
        timeout.as_secs()
    ))
}

/// `cargo xtask bundle [--bin PATH] [--out DIR] [--dmg] [--zip] [--icon PATH]
/// [--sign IDENTITY] [--check]`.
pub fn run(args: &[String]) -> Result<(), String> {
    let out = PathBuf::from(value(args, "out").unwrap_or_else(|| "artifacts/dist".to_owned()));
    let bin = match value(args, "bin") {
        Some(bin) => PathBuf::from(bin),
        None => {
            build_release()?;
            PathBuf::from("target/release").join(EXE_NAME)
        }
    };
    let bin = if bin.is_absolute() {
        bin
    } else {
        workspace_root()?.join(bin)
    };

    let icon = PathBuf::from(value(args, "icon").unwrap_or_else(|| "assets/icon.icns".to_owned()));
    let icon = if icon.is_absolute() {
        icon
    } else {
        workspace_root()?.join(icon)
    };

    let mut options = Options::new(&bin, &out, env!("CARGO_PKG_VERSION"));
    options.icon = Some(icon);
    options.dmg = flag(args, "dmg");
    options.zip = flag(args, "zip");
    options.build = commit_count();
    options.sign = match value(args, "sign") {
        Some(identity) => Sign::Identity(identity),
        None => Sign::AdHoc,
    };

    std::fs::create_dir_all(&options.out).map_err(|error| error.to_string())?;
    let app = assemble(&options)?;
    let stem = artifact_stem(&options.version);

    let mut downloads = Vec::new();
    if options.zip {
        downloads.push(archive(&app, &options.out, &stem)?);
    }
    if options.dmg {
        downloads.push(disk_image(&app, &options.out, &stem)?);
    }
    let sums = if downloads.is_empty() {
        None
    } else {
        Some(checksums(&downloads, &options.out)?)
    };

    let signed = match &options.sign {
        Sign::AdHoc => "ad-hoc signed",
        Sign::Identity(identity) => identity.as_str(),
        #[cfg(test)]
        Sign::None => "unsigned",
    };
    println!(
        "bundled {} {} ({ARCH}, {signed})",
        APP_NAME, options.version
    );
    println!("  {}", app.display());
    for download in downloads {
        println!("  {}", download.display());
    }
    if let Some(sums) = sums {
        println!("  {}", sums.display());
    }

    if flag(args, "check") {
        check(&app, &options.version)?;
        println!("the bundle launches and reports a ready window");
    }
    Ok(())
}

fn build_release() -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--release", "--quiet", "-p", "open-notex"])
        // `--version` names the commit, so even a bundle made here says what it
        // is. The release workflow stamps the same variable.
        .env("NOTEX_REVISION", revision())
        .current_dir(workspace_root()?)
        .status()
        .map_err(|error| format!("could not run cargo: {error}"))?;
    if !status.success() {
        return Err("cargo build --release -p open-notex failed".to_owned());
    }
    Ok(())
}

/// The commit a build came from, or `dev` outside a checkout.
fn revision() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(workspace_root().unwrap_or_default())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|revision| !revision.is_empty())
        .unwrap_or_else(|| "dev".to_owned())
}

/// `CFBundleVersion` only has to increase, and the commit count always does.
/// A shallow clone or a source tarball has no history, so it falls back to 1.
fn commit_count() -> String {
    Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(workspace_root().unwrap_or_default())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|count| !count.is_empty())
        .unwrap_or_else(|| "1".to_owned())
}

fn workspace_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask has no parent directory".to_owned())
}

fn flag(args: &[String], name: &str) -> bool {
    let flag = format!("--{name}");
    args.iter().any(|arg| arg == &flag)
}

fn value(args: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    if let Some(index) = args.iter().position(|arg| arg == &flag) {
        return args.get(index + 1).cloned();
    }
    args.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{flag}=")).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Mach-O header with `cputype`, and nothing else. `assemble` copies the
    /// bytes and reads the header; it never runs them.
    fn macho(cputype: u32) -> Vec<u8> {
        let mut bytes = 0xFEED_FACF_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&cputype.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        bytes
    }

    #[test]
    fn the_plist_names_the_version_it_was_built_with() {
        let plist = info_plist("0.1.0", "42", "openNoteX contributors. MIT licensed.");
        assert!(plist.contains("<key>CFBundleShortVersionString</key>\n\t<string>0.1.0</string>"));
        assert!(plist.contains("<key>CFBundleVersion</key>\n\t<string>42</string>"));
        assert!(plist.contains(&format!("<string>{BUNDLE_ID}</string>")));
        assert!(plist.contains(&format!("<string>{EXE_NAME}</string>")));
        assert!(plist.contains(&format!("<string>{MIN_MACOS}</string>")));
        assert!(plist.contains("<key>NSHighResolutionCapable</key>\n\t<true/>"));
        assert_eq!(
            plist.matches("<dict>").count(),
            plist.matches("</dict>").count()
        );
    }

    #[test]
    fn the_plist_escapes_a_value_that_would_break_the_xml() {
        let plist = info_plist("1.0", "1", "A & B <C> \"D\"");
        assert!(plist.contains("A &amp; B &lt;C&gt; &quot;D&quot;"));
        assert!(!plist.contains("A & B"));
    }

    #[test]
    fn a_binary_is_read_for_its_architecture() {
        assert_eq!(binary_kind(&macho(0x0100_000C)), Binary::Arm64);
        assert_eq!(binary_kind(&macho(0x0100_0007)), Binary::X86_64);
        assert_eq!(binary_kind(&macho(0x0000_000C)), Binary::Unknown);
        assert_eq!(binary_kind(b"\xca\xfe\xba\xbe0000"), Binary::Fat);
        assert_eq!(binary_kind(b"not a mach-o"), Binary::Unknown);
        assert_eq!(binary_kind(b"\xcf\xfa"), Binary::Unknown);
    }

    #[test]
    fn an_intel_binary_is_refused_with_the_command_that_fixes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("open-notex");
        std::fs::write(&path, macho(0x0100_0007)).unwrap();

        let error = check_arch(&path).unwrap_err();
        assert!(error.contains("x86_64"), "{error}");
        assert!(error.contains("Apple Silicon only"), "{error}");
        assert!(error.contains("cargo build --release"), "{error}");
    }

    #[test]
    fn assemble_writes_a_bundle_a_person_can_open() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("open-notex");
        std::fs::write(&bin, macho(0x0100_000C)).unwrap();
        let out = dir.path().join("dist");
        std::fs::create_dir_all(&out).unwrap();

        let mut options = Options::new(&bin, &out, "0.1.0");
        options.sign = Sign::None;
        let app = assemble(&options).unwrap();

        assert_eq!(app.file_name().unwrap(), "openNoteX.app");
        assert!(app.join("Contents/MacOS/open-notex").is_file());
        assert_eq!(
            std::fs::read_to_string(app.join("Contents/PkgInfo")).unwrap(),
            "APPL????"
        );
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>0.1.0</string>"));

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(app.join("Contents/MacOS/open-notex"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the executable is executable");

        // Assembling again replaces the bundle rather than nesting a new one.
        assemble(&options).unwrap();
        assert!(app.join("Contents/Info.plist").is_file());
    }

    #[test]
    fn assemble_refuses_an_intel_binary() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("open-notex");
        std::fs::write(&bin, macho(0x0100_0007)).unwrap();

        let options = Options::new(&bin, dir.path().join("dist"), "0.1.0");
        let error = assemble(&options).unwrap_err();
        assert!(error.contains("Apple Silicon only"), "{error}");
    }

    #[test]
    fn a_missing_icon_says_how_to_make_one() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("open-notex");
        std::fs::write(&bin, macho(0x0100_000C)).unwrap();

        let mut options = Options::new(&bin, dir.path().join("dist"), "0.1.0");
        options.icon = Some(dir.path().join("missing.icns"));
        let error = assemble(&options).unwrap_err();
        assert!(error.contains("make-icon.sh"), "{error}");
    }

    #[test]
    fn an_artifact_is_named_after_the_version_and_the_arch() {
        assert_eq!(artifact_stem("0.1.0"), "openNoteX-0.1.0-arm64");
        assert_eq!(artifact_stem("1.2.3"), "openNoteX-1.2.3-arm64");
    }
}
