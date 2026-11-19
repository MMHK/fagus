//! The binary in its sandbox: what `fagus render` writes, and what a worker may read on Linux.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fagus() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fagus"))
}

/// A new directory for a test.
fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("fagus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");
    directory
}

#[test]
fn writes_the_pdf_only_when_it_renders() {
    let directory = scratch("output");
    let page = directory.join("page.html");
    std::fs::write(&page, "<style>p { colr: red }</style><p>Brief</p>").expect("written");
    let output = directory.join("page.pdf");
    // Strict mode fails on the unknown property: no file is left behind.
    let status = fagus()
        .args(["render", "--strict"])
        .arg(&page)
        .arg("-o")
        .arg(&output)
        .stderr(Stdio::null())
        .status()
        .expect("ran");
    assert!(!status.success());
    assert!(!output.exists(), "an empty file is left");
    // A file that was there keeps its content.
    std::fs::write(&output, "old").expect("written");
    let status = fagus()
        .args(["render", "--strict"])
        .arg(&page)
        .arg("-o")
        .arg(&output)
        .stderr(Stdio::null())
        .status()
        .expect("ran");
    assert!(!status.success());
    assert_eq!(std::fs::read_to_string(&output).expect("read"), "old");
    // Rendered, it replaces a longer file entirely.
    std::fs::write(&output, "x".repeat(1 << 20)).expect("written");
    let status = fagus()
        .arg("render")
        .arg(&page)
        .arg("-o")
        .arg(&output)
        .stderr(Stdio::null())
        .status()
        .expect("ran");
    assert!(status.success());
    let pdf = std::fs::read(&output).expect("read");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(pdf.len() < 1 << 20, "the old content is gone");
    std::fs::remove_dir_all(directory).expect("removed");
}

/// A frame of docs/protocol.md.
#[cfg(target_os = "linux")]
fn frame(kind: u8, header: &str, blobs: &[&[u8]]) -> Vec<u8> {
    let lengths: Vec<String> = blobs.iter().map(|blob| blob.len().to_string()).collect();
    let header = if blobs.is_empty() {
        header.to_owned()
    } else {
        format!(
            "{},\"blobs\":[{}]}}",
            &header[..header.len() - 1],
            lengths.join(",")
        )
    };
    let length = 5 + header.len() + blobs.iter().map(|blob| blob.len()).sum::<usize>();
    let mut frame = (length as u32).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend((header.len() as u32).to_le_bytes());
    frame.extend(header.as_bytes());
    for blob in blobs {
        frame.extend(*blob);
    }
    frame
}

/// The kinds and headers of the frames a worker wrote.
fn frames(mut output: &[u8]) -> Vec<(u8, String)> {
    let mut frames = Vec::new();
    while output.len() >= 9 {
        let length = u32::from_le_bytes(output[..4].try_into().expect("4 bytes")) as usize;
        let header = u32::from_le_bytes(output[5..9].try_into().expect("4 bytes")) as usize;
        frames.push((
            output[4],
            String::from_utf8_lossy(&output[9..9 + header]).into_owned(),
        ));
        output = &output[4 + length..];
    }
    frames
}

/// Runs a worker with `arguments` on `input` and returns its frames.
fn worker(arguments: &[&str], input: &[u8]) -> Vec<(u8, String)> {
    let mut child = fagus()
        .arg("worker")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("started");
    child
        .stdin
        .take()
        .expect("input")
        .write_all(input)
        .expect("written");
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .expect("output")
        .read_to_end(&mut output)
        .expect("read");
    assert!(child.wait().expect("ended").success());
    frames(&output)
}

/// Whether the tests must find Landlock at work (`FAGUS_REQUIRE_LANDLOCK=1`, as in CI).
fn landlock_required() -> bool {
    std::env::var_os("FAGUS_REQUIRE_LANDLOCK").is_some()
}

#[test]
fn says_whether_it_is_sandboxed() {
    let hello = &worker(&[], b"")[0];
    assert_eq!(hello.0, b'H');
    if cfg!(target_os = "linux") {
        if landlock_required() {
            assert!(hello.1.contains(r#""sandbox":"on""#), "{}", hello.1);
        }
    } else {
        assert!(hello.1.contains(r#""sandbox":"off""#), "{}", hello.1);
    }
    let hello = &worker(&["--no-sandbox"], b"")[0];
    assert!(hello.1.contains(r#""sandbox":"off""#), "{}", hello.1);
}

/// A worker told to read only one directory cannot read a document's files elsewhere, even where
/// the caller allows them: the sandbox stands behind the checks of the engine.
#[cfg(target_os = "linux")]
#[test]
fn reads_only_the_allowed_directories() {
    let allowed = scratch("allowed");
    let elsewhere = scratch("elsewhere");
    std::fs::write(elsewhere.join("style.css"), "p { colr: red }").expect("written");
    let request = frame(
        b'R',
        &format!(
            r#"{{"id":1,"html":0,"base":"{}","created":"2026-01-01T12:00:00Z"}}"#,
            elsewhere.display()
        ),
        &[br#"<link rel="stylesheet" href="style.css"><p>x</p>"#],
    );
    // Outside the sandbox the style sheet is read (and its unknown property reported).
    let free = worker(&["--no-sandbox"], &request);
    assert_eq!(free[1].0, b'P');
    assert!(free[1].1.contains("css.unknown-property"), "{}", free[1].1);
    assert!(!free[1].1.contains("resource.not-loaded"), "{}", free[1].1);
    let confined = worker(&["--allow-read", allowed.to_str().expect("UTF-8")], &request);
    assert_eq!(confined[1].0, b'P');
    if confined[0].1.contains(r#""sandbox":"on""#) || landlock_required() {
        assert!(confined[1].1.contains("resource.not-loaded"), "{}", confined[1].1);
        assert!(
            !confined[1].1.contains("css.unknown-property"),
            "{}",
            confined[1].1
        );
    }
    std::fs::remove_dir_all(allowed).expect("removed");
    std::fs::remove_dir_all(elsewhere).expect("removed");
}

#[cfg(target_os = "linux")]
#[test]
fn limits_the_memory() {
    let directory = scratch("memory");
    let page = directory.join("page.html");
    std::fs::write(&page, "<p>Brief</p>").expect("written");
    let render = |limit: &str| {
        fagus()
            .args(["render", "--memory-limit", limit])
            .arg(&page)
            .arg("-o")
            .arg(directory.join("page.pdf"))
            .stderr(Stdio::null())
            .status()
            .expect("ran")
            .success()
    };
    assert!(render("65536"), "64 GiB are enough");
    assert!(!render("1"), "1 MiB is not");
    std::fs::remove_dir_all(directory).expect("removed");
}

#[cfg(not(target_os = "linux"))]
#[test]
fn limits_only_on_linux() {
    let output = fagus()
        .args(["render", "--memory-limit", "1024", "page.html", "-o", "page.pdf"])
        .output()
        .expect("ran");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only supported on Linux"));
}
