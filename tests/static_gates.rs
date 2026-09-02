#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard, OnceLock};

use tempfile::TempDir;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_script(
    script: &str,
    working_directory: &Path,
    arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Output {
    Command::new("bash")
        .arg(repository_root().join("scripts").join(script))
        .args(arguments)
        .current_dir(working_directory)
        .output()
        .expect("static gate script must run")
}

fn assert_rejected(output: Output, gate: &str) {
    assert!(
        !output.status.success(),
        "{gate} accepted its negative fixture\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clippy_policy_paths(policy: &str) -> (BTreeSet<&str>, BTreeSet<&str>) {
    let mut section = "";
    let mut methods = BTreeSet::new();
    let mut types = BTreeSet::new();

    for line in policy.lines() {
        match line.trim() {
            "disallowed-methods = [" => section = "methods",
            "disallowed-types = [" => section = "types",
            "]" => section = "",
            entry if entry.starts_with("{ path = \"") => {
                let path = entry
                    .split('"')
                    .nth(1)
                    .expect("Clippy policy path must be quoted");
                match section {
                    "methods" => {
                        methods.insert(path);
                    }
                    "types" => {
                        types.insert(path);
                    }
                    _ => panic!("Clippy policy path is outside a known section"),
                }
            }
            _ => {}
        }
    }

    (methods, types)
}

#[test]
fn clippy_disallowed_policy_is_an_exact_reviewed_set() {
    const METHODS: &[&str] = &[
        "std::fs::DirBuilder::create",
        "std::fs::File::create",
        "std::fs::File::create_new",
        "std::fs::File::set_len",
        "std::fs::File::set_modified",
        "std::fs::File::set_permissions",
        "std::fs::File::set_times",
        "std::fs::OpenOptions::append",
        "std::fs::OpenOptions::create",
        "std::fs::OpenOptions::create_new",
        "std::fs::OpenOptions::truncate",
        "std::fs::OpenOptions::write",
        "std::fs::copy",
        "std::fs::create_dir",
        "std::fs::create_dir_all",
        "std::fs::hard_link",
        "std::fs::remove_dir",
        "std::fs::remove_dir_all",
        "std::fs::remove_file",
        "std::fs::rename",
        "std::fs::set_permissions",
        "std::fs::soft_link",
        "std::fs::write",
        "std::net::TcpListener::accept",
        "std::net::TcpListener::bind",
        "std::net::TcpListener::incoming",
        "std::net::TcpStream::connect",
        "std::net::TcpStream::connect_timeout",
        "std::net::ToSocketAddrs::to_socket_addrs",
        "std::net::UdpSocket::bind",
        "std::net::UdpSocket::connect",
        "std::os::unix::fs::FileExt::write_all_at",
        "std::os::unix::fs::FileExt::write_at",
        "std::os::unix::fs::OpenOptionsExt::custom_flags",
        "std::os::unix::fs::chown",
        "std::os::unix::fs::fchown",
        "std::os::unix::fs::lchown",
        "std::os::unix::fs::symlink",
        "std::os::unix::net::UnixDatagram::bind",
        "std::os::unix::net::UnixDatagram::bind_addr",
        "std::os::unix::net::UnixDatagram::connect",
        "std::os::unix::net::UnixDatagram::connect_addr",
        "std::os::unix::net::UnixDatagram::pair",
        "std::os::unix::net::UnixDatagram::unbound",
        "std::os::unix::net::UnixListener::accept",
        "std::os::unix::net::UnixListener::bind",
        "std::os::unix::net::UnixListener::bind_addr",
        "std::os::unix::net::UnixListener::incoming",
        "std::os::unix::net::UnixStream::connect",
        "std::os::unix::net::UnixStream::connect_addr",
        "std::os::unix::net::UnixStream::pair",
    ];
    const TYPES: &[&str] = &["std::process::Command"];
    let policy = fs::read_to_string(repository_root().join("clippy.toml"))
        .expect("Clippy policy must be readable");
    let (methods, types) = clippy_policy_paths(&policy);

    assert_eq!(methods, METHODS.iter().copied().collect());
    assert_eq!(types, TYPES.iter().copied().collect());
}

#[test]
fn fsx_extern_symbols_are_an_exact_reviewed_set() {
    let source = fs::read_to_string(repository_root().join("src/fsx/sys.rs"))
        .expect("fsx syscall boundary must be readable");
    let mut inside_extern = false;
    let mut declared = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        if line == "unsafe extern \"C\" {" {
            inside_extern = true;
        } else if inside_extern && line == "}" {
            inside_extern = false;
        } else if inside_extern
            && let Some(declaration) = line.strip_prefix("fn ")
            && let Some((name, _)) = declaration.split_once('(')
        {
            declared.insert(name);
        } else if inside_extern
            && let Some(declaration) = line.strip_prefix("static ")
            && let Some((name, _)) = declaration.split_once(':')
        {
            declared.insert(name);
        }
    }
    let expected = BTreeSet::from([
        "CFNumberGetValue",
        "CFRelease",
        "CFURLCopyResourcePropertyForKey",
        "CFURLCreateFromFileSystemRepresentation",
        "fstatfs",
        "getattrlist",
        "getiopolicy_np",
        "fs_snapshot_list",
        "kCFURLVolumeAvailableCapacityForImportantUsageKey",
        "kCFURLVolumeAvailableCapacityForOpportunisticUsageKey",
        "setiopolicy_np",
        "statfs",
    ]);

    assert_eq!(declared, expected);
}

#[test]
fn x86_statfs_symbols_are_locked_to_the_modern_inode64_abi() {
    let source = fs::read_to_string(repository_root().join("src/fsx/sys.rs"))
        .expect("fsx syscall boundary must be readable");
    for symbol in ["statfs", "fstatfs"] {
        assert!(
            source.contains(&format!(
                "#[cfg_attr(target_arch = \"x86_64\", link_name = \"{symbol}$INODE64\")]"
            )),
            "x86_64 {symbol} must retain the SDK's modern inode64 symbol alias"
        );
    }
}

#[test]
fn no_build_script_or_direct_libc_dependency_opens_an_unchecked_write_surface() {
    assert!(
        !repository_root().join("build.rs").exists(),
        "a build script is a separate crate and needs the full zero-write policy"
    );
    let manifest = fs::read_to_string(repository_root().join("Cargo.toml"))
        .expect("Cargo manifest must be readable");
    let dependencies = manifest
        .split_once("[dependencies]")
        .and_then(|(_, tail)| tail.split_once('[').map(|(section, _)| section))
        .expect("manifest must contain a dependencies section");
    assert!(
        !dependencies
            .lines()
            .any(|line| line.trim_start().starts_with("libc")),
        "declaring libc would make the complete write syscall surface linkable"
    );
}

#[test]
fn fsx_cannot_bypass_the_locked_symbol_set() {
    let source = fs::read_to_string(repository_root().join("src/fsx/sys.rs"))
        .expect("fsx syscall boundary must be readable");
    for bypass in ["asm!", "global_asm!", "dlsym", "dlopen", "syscall("] {
        assert!(
            !source.contains(bypass),
            "fsx uses {bypass}, bypassing the reviewed extern symbol set"
        );
    }
}

#[test]
fn crate_roots_forbid_disallowed_methods() {
    for crate_root in ["src/lib.rs", "src/main.rs"] {
        let source = fs::read_to_string(repository_root().join(crate_root))
            .expect("crate root must be readable");
        assert_eq!(
            source.lines().next(),
            Some("#![forbid(clippy::disallowed_methods)]"),
            "{crate_root} must make the zero-write lint impossible to downgrade"
        );
    }
}

#[test]
fn zero_write_gate_rejects_file_set_times() {
    let fixture = TempDir::new().expect("temporary crate must be created");
    fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
    fs::copy(
        repository_root().join("clippy.toml"),
        fixture.path().join("clippy.toml"),
    )
    .expect("clippy policy must be copied");
    fs::write(
        fixture.path().join("Cargo.toml"),
        "[package]\nname = \"zero-write-mutation\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .expect("fixture manifest must be written");
    fs::write(
        fixture.path().join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"zero-write-mutation\"\nversion = \"0.0.0\"\n",
    )
    .expect("fixture lockfile must be written");
    fs::write(
        fixture.path().join("src/main.rs"),
        "#![forbid(clippy::disallowed_methods)]\n#[allow(clippy::disallowed_methods)]\nfn main() {\n    let file = std::fs::File::open(\"artifact.bin\").unwrap();\n    let times = std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH);\n    file.set_times(times).unwrap();\n}\n",
    )
    .expect("mutation source must be written");

    let output = Command::new("cargo")
        .args(["clippy", "--locked", "--", "-D", "warnings"])
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .current_dir(fixture.path())
        .output()
        .expect("fixture clippy must run");

    assert_rejected(output, "zero-write Clippy gate");
}

#[test]
fn lint_boundary_gate_rejects_all_suppression_forms_outside_policy() {
    let forms = [
        "#![expect(clippy::disallowed_types)]\n",
        "#![cfg_attr(test, allow(clippy::disallowed_types))]\n",
        "#![allow(clippy::disallowed_types)]\n",
        "#![allow(clippy::style)]\n",
        "#![expect(clippy::all)]\n",
        "#![allow(clippy::disallowed_type)]\n",
    ];

    for form in forms {
        let fixture = TempDir::new().expect("boundary fixture must be created");
        fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
        fs::write(
            fixture.path().join("src/policy.rs"),
            "#![allow(clippy::disallowed_types)]\n",
        )
        .expect("allowed suppression must be written");
        fs::write(fixture.path().join("src/scan.rs"), form)
            .expect("forbidden suppression must be written");

        let output = run_script(
            "check-policy-boundary.sh",
            fixture.path(),
            [
                "clippy::disallowed_types",
                "src/policy.rs",
                "src",
                "clippy::style",
                "clippy::all",
                "clippy::disallowed_type",
                "warnings",
            ],
        );
        assert_rejected(output, "lint suppression boundary gate");
    }

    let fixture = TempDir::new().expect("unsafe boundary fixture must be created");
    fs::create_dir_all(fixture.path().join("src/fsx")).expect("fsx directory must be created");
    fs::write(
        fixture.path().join("src/fsx/sys.rs"),
        "#![allow(unsafe_code)]\n",
    )
    .expect("allowed unsafe suppression must be written");
    fs::write(
        fixture.path().join("src/scan.rs"),
        "#![cfg_attr(test, expect(unsafe_code))]\n",
    )
    .expect("forbidden unsafe suppression must be written");

    let output = run_script(
        "check-policy-boundary.sh",
        fixture.path(),
        ["unsafe_code", "src/fsx/sys.rs", "src", "warnings"],
    );
    assert_rejected(output, "unsafe suppression boundary gate");

    for form in [
        "#![allow(clippy::disallowed_methods)]\n",
        "#![expect(clippy::disallowed_methods)]\n",
        "#![cfg_attr(test, allow(clippy::disallowed_methods))]\n",
    ] {
        let fixture = TempDir::new().expect("method boundary fixture must be created");
        fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
        fs::write(fixture.path().join("src/scan.rs"), form)
            .expect("forbidden method suppression must be written");

        let output = run_script(
            "check-policy-boundary.sh",
            fixture.path(),
            ["clippy::disallowed_methods", "-", "src"],
        );
        assert_rejected(output, "disallowed-methods suppression boundary gate");
    }
}

#[test]
fn lint_boundary_gate_accepts_no_disallowed_methods_suppression() {
    let fixture = TempDir::new().expect("method boundary fixture must be created");
    fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
    fs::write(fixture.path().join("src/scan.rs"), "pub fn scan() {}\n")
        .expect("clean source must be written");

    let output = run_script(
        "check-policy-boundary.sh",
        fixture.path(),
        ["clippy::disallowed_methods", "-", "src"],
    );
    assert!(
        output.status.success(),
        "zero-exemption boundary rejected clean source\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn claim_pattern_gate_rejects_forbidden_program_output() {
    let fixture = TempDir::new().expect("claim fixture must be created");
    fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
    fs::write(
        fixture.path().join("src/main.rs"),
        "fn main() { println!(\"释放 87 GB\"); }\n",
    )
    .expect("forbidden public string must be written");

    assert_rejected(
        run_script("check-claim-patterns.sh", fixture.path(), [] as [&str; 0]),
        "truth-contract claim gate",
    );
}

#[test]
fn quantitative_document_gate_rejects_handwritten_number() {
    let fixture = TempDir::new().expect("documentation fixture must be created");
    fs::create_dir(fixture.path().join("docs")).expect("docs directory must be created");
    fs::write(fixture.path().join("docs/guide.md"), "Observed 87 GB.\n")
        .expect("handwritten number must be written");

    assert_rejected(
        run_script(
            "check-quantitative-docs.sh",
            fixture.path(),
            [] as [&str; 0],
        ),
        "quantitative documentation gate",
    );
}

#[test]
fn quantitative_document_gate_rejects_a_forged_generated_fragment() {
    let fixture = TempDir::new().expect("fragment fixture must be created");
    fs::create_dir_all(fixture.path().join("docs/generated"))
        .expect("generated directory must be created");
    fs::create_dir(fixture.path().join("scripts")).expect("scripts directory must be created");
    fs::copy(
        repository_root().join("scripts/sync-generated-fragments.py"),
        fixture.path().join("scripts/sync-generated-fragments.py"),
    )
    .expect("fragment verifier must be copied");
    fs::write(
        fixture.path().join("docs/generated/support-matrix.md"),
        "verified source\n",
    )
    .expect("support fragment must be written");
    fs::write(
        fixture.path().join("docs/generated/fixture-report.md"),
        "verified fixture\n",
    )
    .expect("report fragment must be written");
    fs::write(
        fixture.path().join("docs/generated/measurement-basis.md"),
        "verified basis\n",
    )
    .expect("basis fragment must be written");
    fs::write(
        fixture
            .path()
            .join("docs/generated/coverage-unknown-baseline.md"),
        "verified baseline\n",
    )
    .expect("baseline fragment must be written");
    fs::write(
        fixture.path().join("README.md"),
        "<!-- BEGIN GENERATED: support-matrix -->\nforged 87 GB\n<!-- END GENERATED: support-matrix -->\n<!-- BEGIN GENERATED: fixture-report -->\nverified fixture\n<!-- END GENERATED: fixture-report -->\n<!-- BEGIN GENERATED: measurement-basis -->\nverified basis\n<!-- END GENERATED: measurement-basis -->\n<!-- BEGIN GENERATED: coverage-unknown-baseline -->\nverified baseline\n<!-- END GENERATED: coverage-unknown-baseline -->\n",
    )
    .expect("forged README must be written");

    assert_rejected(
        run_script(
            "check-quantitative-docs.sh",
            fixture.path(),
            [] as [&str; 0],
        ),
        "generated fragment transclusion gate",
    );
}

#[test]
fn locked_metadata_gate_rejects_a_stale_lockfile() {
    let fixture = TempDir::new().expect("metadata fixture must be created");
    fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
    fs::write(fixture.path().join("src/lib.rs"), "").expect("fixture source must be written");
    fs::write(
        fixture.path().join("Cargo.toml"),
        "[package]\nname = \"stale-lock\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n\n[workspace]\n",
    )
    .expect("fixture manifest must be written");
    fs::write(
        fixture.path().join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"stale-lock\"\nversion = \"0.0.0\"\n",
    )
    .expect("stale lockfile must be written");

    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(fixture.path())
        .output()
        .expect("locked metadata gate must run");

    assert_rejected(output, "locked metadata gate");
}

#[test]
fn generated_document_gate_rejects_a_hand_edited_file() {
    let fixture = TempDir::new().expect("generated document fixture must be created");
    fs::create_dir_all(fixture.path().join("docs/generated"))
        .expect("generated directory must be created");
    Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(fixture.path())
        .status()
        .expect("fixture repository must initialize");
    fs::write(fixture.path().join("docs/generated/example.json"), "{}\n")
        .expect("baseline generated file must be written");
    Command::new("git")
        .args(["add", "docs/generated/example.json"])
        .current_dir(fixture.path())
        .status()
        .expect("baseline file must be staged");
    fs::write(
        fixture.path().join("docs/generated/example.json"),
        "{\"hand_edited\":true}\n",
    )
    .expect("generated file must be mutated");

    assert_rejected(
        run_script("check-generated-docs.sh", fixture.path(), [] as [&str; 0]),
        "generated documentation drift gate",
    );
}

#[test]
fn minimum_macos_gate_rejects_newer_deployment_target() {
    let fixture = TempDir::new().expect("Mach-O fixture must be created");
    let tools = fixture.path().join("bin");
    fs::create_dir(&tools).expect("fake tool directory must be created");
    let lipo = tools.join("lipo");
    let otool = tools.join("otool");
    fs::write(&lipo, "#!/bin/sh\necho arm64\n").expect("fake lipo must be written");
    fs::write(
        &otool,
        "#!/bin/sh\necho '      cmd LC_BUILD_VERSION'\necho '    minos 14.0'\n",
    )
    .expect("fake otool must be written");
    fs::set_permissions(&lipo, fs::Permissions::from_mode(0o755))
        .expect("fake lipo must be executable");
    fs::set_permissions(&otool, fs::Permissions::from_mode(0o755))
        .expect("fake otool must be executable");
    let binary = fixture.path().join("sizetrail");
    fs::write(&binary, []).expect("placeholder binary must be written");

    let output = Command::new("bash")
        .arg(repository_root().join("scripts/check-minimum-macos.sh"))
        .args([binary.as_os_str(), OsStr::new("arm64")])
        .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
        .current_dir(fixture.path())
        .output()
        .expect("minimum macOS gate must run");

    assert_rejected(output, "minimum macOS gate");
}

/// A document that satisfies every `scan` assertion, so a negative fixture built on it can only be
/// rejected for the one property under test. `scan` is the first command the gate observes, so a
/// fixture meant to be rejected never reaches the later subcommands.
const COMPLETE_ENOUGH_DOCUMENT: &str =
    r#"{"schema_version":"1.0.0","payload":{"regions":[{"id":"capacity","status":"complete"}]}}"#;

/// A stand-in for the whole CLI surface, needed by fixtures that must pass the gate rather than be
/// rejected by it: the sandbox observes every advertised subcommand, so a fake that answers only
/// `scan` fails on the next one (Q49). Returning 42 when the probe kill-switch is absent is what
/// proves the gate sets it.
const CLI_SURFACE_FIXTURE: &str = r##"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    const char *xcode_gate = getenv("SIZETRAIL_NO_XCODE_PROBE");
    const char *docker_gate = getenv("SIZETRAIL_NO_DOCKER_PROBE");
    const char *go_gate = getenv("SIZETRAIL_NO_GO_PROBE");
    if (xcode_gate == NULL || strcmp(xcode_gate, "1") != 0
        || docker_gate == NULL || strcmp(docker_gate, "1") != 0
        || go_gate == NULL || strcmp(go_gate, "1") != 0) {
        return 42;
    }

    const char *command = argc > 1 ? argv[1] : "";

    if (strcmp(command, "scan") == 0) {
        puts("{\"schema_version\":\"1.0.0\",\"payload\":"
             "{\"regions\":[{\"id\":\"capacity\",\"status\":\"complete\"}],"
             "\"findings\":[{\"rule_id\":\"docker.virtual_disk\"},"
             "{\"rule_id\":\"go.build_cache\"}]}}");
        return 0;
    }
    if (strcmp(command, "doctor") == 0) {
        puts("{\"side_effect_policy\":[],\"root\":{\"status\":\"readable\"}}");
        return 0;
    }
    if (strcmp(command, "rules") == 0) {
        puts("[{\"evidence\":\"fixture rule\"}]");
        return 0;
    }
    if (strcmp(command, "completion") == 0) {
        puts("#compdef sizetrail");
        return 0;
    }
    if (strcmp(command, "explain") == 0) {
        fputs("sizetrail: finding is absent from the supplied report\n", stderr);
        return 1;
    }
    if (strcmp(command, "--version") == 0) {
        puts("sizetrail 0.0.0-fixture");
        return 0;
    }

    puts("Commands:\n  scan\n  doctor\n  rules\n  completion\n  explain");
    return 0;
}
"##;

fn write_fake_binary(directory: &Path, name: &str, body: &str) -> PathBuf {
    let binary = directory.join(name);
    fs::write(&binary, body).expect("fake binary must be written");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .expect("fake binary must be executable");
    binary
}

fn sandbox_gate_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn sandbox_gate_rejects_a_swallowed_hard_coded_write() {
    let _serial = sandbox_gate_lock();
    let fixture = TempDir::new().expect("sandbox fixture must be created");
    let fake_binary = write_fake_binary(
        fixture.path(),
        "sizetrail-mutation",
        &format!(
            "#!/bin/sh\n: > /tmp/sizetrail-smuggled-sandbox-probe 2>/dev/null || true\nprintf '%s\\n' '{COMPLETE_ENOUGH_DOCUMENT}'\nexit 0\n"
        ),
    );

    assert_rejected(
        run_script(
            "check-zero-write-sandbox.sh",
            fixture.path(),
            [fake_binary.as_os_str()],
        ),
        "sandbox write-attempt gate",
    );
}

#[test]
fn sandbox_gate_rejects_a_scan_that_measured_nothing() {
    let _serial = sandbox_gate_lock();
    let fixture = TempDir::new().expect("sandbox fixture must be created");
    let fake_binary = write_fake_binary(
        fixture.path(),
        "sizetrail-idle",
        "#!/bin/sh\nprintf '%s\\n' '{\"schema_version\":\"1.0.0\",\"payload\":{\"regions\":[{\"id\":\"capacity\",\"status\":\"unmeasurable\"}]}}'\nexit 0\n",
    );

    assert_rejected(
        run_script(
            "check-zero-write-sandbox.sh",
            fixture.path(),
            [fake_binary.as_os_str()],
        ),
        "sandbox measurement-liveness gate",
    );
}

#[test]
fn sandbox_gate_disables_registered_external_probes_without_skipping_measurement() {
    let _serial = sandbox_gate_lock();
    let fixture = TempDir::new().expect("sandbox fixture must be created");
    let source = fixture.path().join("probe_boundary.c");
    let fake_binary = fixture.path().join("sizetrail-probe-boundary");
    fs::write(&source, CLI_SURFACE_FIXTURE).expect("probe-boundary fixture source must be written");
    let compiled = Command::new("xcrun")
        .args(["clang", "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-o")
        .arg(&fake_binary)
        .status()
        .expect("probe-boundary fixture compiler must run");
    assert!(compiled.success(), "probe-boundary fixture must compile");

    let output = run_script(
        "check-zero-write-sandbox.sh",
        fixture.path(),
        [fake_binary.as_os_str()],
    );
    assert!(
        output.status.success(),
        "product-process sandbox did not close the registered external-probe boundary\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn apfs_counterexample_construction_exceptions_stay_empty() {
    let source = fs::read_to_string(repository_root().join("tests/apfs_counterexamples.rs"))
        .expect("counterexample tests must be readable");

    assert!(
        source.contains("const CONSTRUCTION_EXCEPTIONS: &[&str] = &[];"),
        "a counterexample construction exception was added; every entry silently removes \
         empirical evidence for the non-convergence rule and needs a decision record"
    );
}

/// Q49: the sandbox proved only `scan` for three phases while the coverage matrix described the
/// result as a property of the product. Enumerate what the binary actually offers so that adding a
/// subcommand without observing it fails here instead of quietly widening the claim.
#[test]
fn the_zero_write_sandbox_exercises_every_subcommand_the_binary_offers() {
    let root = repository_root();
    let help = Command::new(env!("CARGO_BIN_EXE_sizetrail"))
        .arg("--help")
        .output()
        .expect("help must run");
    let advertised = String::from_utf8_lossy(&help.stdout);

    let mut subcommands = BTreeSet::new();
    let mut inside_commands = false;
    for line in advertised.lines() {
        if line.starts_with("Commands:") {
            inside_commands = true;
            continue;
        }
        if inside_commands {
            if !line.starts_with("  ") {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                subcommands.insert(name.to_owned());
            }
        }
    }
    subcommands.remove("help");
    assert!(
        subcommands.len() >= 5,
        "failed to parse the advertised subcommands: {subcommands:?}"
    );

    let script = fs::read_to_string(root.join("scripts/check-zero-write-sandbox.sh"))
        .expect("sandbox gate must be readable");
    for name in &subcommands {
        assert!(
            script.contains(&format!("run_product {name} "))
                || script.contains(&format!("run_product {name}\n")),
            "the zero-write sandbox never runs `{name}`, so the claim does not cover it"
        );
    }
    assert!(
        script.contains("--version"),
        "the version flag runs the binary and must be observed too"
    );
}
