// SPDX-License-Identifier: MIT OR Apache-2.0
//! Record names as host file names.
//!
//! A record name ([`unemu_core_api::is_record_name`]) is already a portable
//! file name: lower-case ASCII letters, digits, `-` and `.`, ending in a
//! letter or a digit, so no file system folds two names into one. Only
//! Windows still treats some of them specially: `con`, `prn`, `aux`,
//! `nul`, `com0` to `com9` and `lpt0` to `lpt9` name devices, also with
//! any extension (`nul.bin`, `com1.a.b`), in every directory.
//!
//! The mapping, the same on every host so that a directory moves between
//! them: when the part of the name before its first dot is one of those
//! device names, the file name is the name with `_` inserted after that
//! part (`con` is `con_`, `nul.bin` is `nul_.bin`). Every other name is
//! its own file name, except for sidecar conflicts below.
//!
//! A name ending in `.lock` or containing `.create-` or `.retain-` conflicts
//! with path-record sidecars. For those names, replace every dot with `_`
//! and surround the result with `_`: `record.lock` becomes `_record_lock_`.
//! This takes precedence over device mapping. A record name never holds `_`,
//! so the mapping is reversible and cannot collide with ordinary or device
//! mappings. Previously usable names keep exactly their existing filenames;
//! escaped names were previously rejected, so existing stores need no migration.
//!
//! The storage's own files beside the records hold `_` in another place:
//! `FILE_lock` (the record's lock) and `FILE_N.tmp` (a replacement while it
//! is written, `N` a decimal number), where `FILE` is the record's file
//! name. [`record_name`] answers `None` for both.

use unemu_core_api::is_record_name;

/// The host file name of the record `name`, or `None` when `name` is not a
/// record name.
pub fn file_name(name: &str) -> Option<String> {
    if !is_record_name(name) {
        return None;
    }
    if crate::path::is_reserved_path(std::path::Path::new(name)) {
        return Some(format!("_{}_", name.replace('.', "_")));
    }
    let (stem, rest) = split_stem(name);
    if is_device_stem(stem) {
        Some(format!("{stem}_{rest}"))
    } else {
        Some(name.to_owned())
    }
}

/// The record name a host file name stands for, or `None` when the file is
/// not a record's: the storage's lock and temporary files, and any name
/// [`file_name`] never makes.
pub fn record_name(file: &str) -> Option<String> {
    let name = if let Some(escaped) = file.strip_prefix('_').and_then(|s| s.strip_suffix('_')) {
        escaped.replace('_', ".")
    } else {
        match file.split_once('_') {
            Some((stem, rest)) if is_device_stem(stem) => format!("{stem}{rest}"),
            Some(_) => return None,
            None => file.to_owned(),
        }
    };
    (file_name(&name).as_deref() == Some(file)).then_some(name)
}

/// The name before its first dot, and the rest from that dot on.
fn split_stem(name: &str) -> (&str, &str) {
    match name.find('.') {
        Some(index) => name.split_at(index),
        None => (name, ""),
    }
}

/// Whether Windows takes `stem` for a device, whatever the extension
/// ("Naming Files, Paths, and Namespaces" of the Win32 documentation, the
/// reserved names). The documentation lists the digits 1 to 9 and, in its
/// later versions, 0 as well; mapping both costs nothing.
fn is_device_stem(stem: &str) -> bool {
    match stem.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_get_an_underscore_after_the_device_part() {
        let cases = [
            ("con", "con_"),
            ("prn", "prn_"),
            ("aux", "aux_"),
            ("nul", "nul_"),
            ("com0", "com0_"),
            ("com1", "com1_"),
            ("lpt9", "lpt9_"),
            ("nul.bin", "nul_.bin"),
            ("com1.a.b", "com1_.a.b"),
        ];
        for (name, file) in cases {
            assert_eq!(file_name(name).as_deref(), Some(file), "{name}");
            assert_eq!(record_name(file).as_deref(), Some(name), "{file}");
        }
    }

    #[test]
    fn other_names_are_their_own_file_names() {
        for name in [
            "a", "0", "cart.sav", "console", "conx", "acon", "com10", "lpt", "com", "nul-1",
            "x.con", "disk.nul", "aux1", "kept-a.b",
        ] {
            assert_eq!(file_name(name).as_deref(), Some(name), "{name}");
            assert_eq!(record_name(name).as_deref(), Some(name), "{name}");
        }
    }

    #[test]
    fn invalid_record_names_have_no_file_name() {
        for name in [
            "",
            "A",
            "a.",
            "-a",
            ".a",
            "a_b",
            "a/b",
            "a b",
            &"a".repeat(65),
        ] {
            assert_eq!(file_name(name), None, "{name:?}");
        }
    }

    #[test]
    fn the_storage_s_own_files_and_raw_device_names_are_no_records() {
        for file in [
            "cart.sav_lock",
            "cart.sav_0.tmp",
            "con__lock",
            "con__3.tmp",
            "nul_.bin_lock",
            "con",
            "nul.bin",
            "com1.a",
            "a_b",
            "nul_x",
            "con_.",
            "Cart",
        ] {
            assert_eq!(record_name(file), None, "{file}");
        }
    }

    /// Every name of up to three bytes of a small alphabet that reaches all
    /// the cases, plus every device form: the mapping is reversible, never
    /// joins two names, and never yields a file name Windows takes for a
    /// device.
    #[test]
    fn the_mapping_round_trips_and_is_injective() {
        let alphabet = ["a", "c", "o", "n", "u", "l", "1", "-", "."];
        let mut names = std::collections::BTreeSet::new();
        for first in alphabet {
            names.insert(first.to_owned());
            for second in alphabet {
                names.insert(format!("{first}{second}"));
                for third in alphabet {
                    names.insert(format!("{first}{second}{third}"));
                }
            }
        }
        for stem in ["con", "prn", "aux", "nul", "com0", "com9", "lpt0", "lpt9"] {
            for rest in ["", ".a", ".nul", ".a.b", ".-1"] {
                names.insert(format!("{stem}{rest}"));
            }
        }
        for stem in ["record", "con", "nul", "com9", "a.b"] {
            for rest in [
                ".lock",
                ".create-0.tmp",
                ".retain-9.tmp",
                ".create-x",
                ".retain-x.lock",
            ] {
                names.insert(format!("{stem}{rest}"));
            }
        }
        names.insert(format!("{}.lock", "a".repeat(59)));
        let mut files = std::collections::BTreeSet::new();
        for name in names.iter().filter(|name| is_record_name(name)) {
            let file = file_name(name).expect("a record name has a file name");
            assert!(!is_device_stem(split_stem(&file).0), "{file}");
            assert!(
                !crate::path::is_reserved_path(std::path::Path::new(&file)),
                "{file}"
            );
            assert_eq!(record_name(&file).as_deref(), Some(name.as_str()));
            assert!(files.insert(file), "{name} shares its file name");
        }
        assert!(files.len() > 300, "the sample covers the alphabet");
    }
}
