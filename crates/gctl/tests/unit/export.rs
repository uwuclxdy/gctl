#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The export write's fallback for filesystems that refuse hard links (vfat,
//! exfat), driven through injected link and fill steps.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use super::write_export_with;
use crate::CmdError;

fn refuse_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::PermissionDenied.into())
}

fn unsupported_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

fn real_link(from: &Path, to: &Path) -> io::Result<()> {
    fs::hard_link(from, to)
}

fn fill_all(file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

#[test]
fn without_hard_links_the_export_is_written_directly() {
    for link in [refuse_link, unsupported_link] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.toml");
        let mut fill = fill_all;
        let result = write_export_with(&path, "format = 1\n", link, &mut fill);
        assert!(result.is_ok(), "the fallback writes the file");
        assert_eq!(fs::read_to_string(&path).unwrap(), "format = 1\n");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no temp file left beside the export"
        );
    }
}

#[test]
fn a_failed_direct_write_leaves_no_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.toml");
    let mut calls = 0;
    // the first fill is the temp file; the second, the fallback's target, fails halfway
    let mut fill = |file: &mut fs::File, bytes: &[u8]| {
        calls += 1;
        if calls == 1 {
            fill_all(file, bytes)
        } else {
            file.write_all(&bytes[..3])?;
            Err(io::Error::other("disk full"))
        }
    };
    let result = write_export_with(&path, "format = 1\n", refuse_link, &mut fill);
    match result {
        Err(CmdError::Message(message)) => {
            assert_eq!(message, format!("could not write {path:?}: disk full"));
        }
        _ => panic!("a failed write must be an error naming the path"),
    }
    assert_eq!(calls, 2, "the temp file, then the target");
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        0,
        "neither the partial target nor the temp file is left"
    );
}

/// Another process creates the target between the exists check and the link.
fn raced_link(_: &Path, to: &Path) -> io::Result<()> {
    fs::write(to, "theirs")?;
    Err(io::ErrorKind::AlreadyExists.into())
}

#[test]
fn a_target_that_appears_before_the_link_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.toml");
    let mut fill = fill_all;
    let result = write_export_with(&path, "format = 1\n", raced_link, &mut fill);
    match result {
        Err(CmdError::Message(message)) => assert_eq!(message, format!("{path:?} exists")),
        _ => panic!("a raced target must be refused"),
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "the temp file is gone, their file stays"
    );
}

#[test]
fn a_failed_temp_write_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.toml");
    let mut fill = |_: &mut fs::File, _: &[u8]| Err(io::Error::other("disk full"));
    let result = write_export_with(&path, "format = 1\n", real_link, &mut fill);
    match result {
        Err(CmdError::Message(message)) => {
            assert_eq!(message, format!("could not write {path:?}: disk full"));
        }
        _ => panic!("a failed write must be an error naming the path"),
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
