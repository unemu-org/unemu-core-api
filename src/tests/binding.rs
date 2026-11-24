// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use crate::test_support::{check_prepared, ScriptedBindingCore};
use std::cell::RefCell;
use std::collections::BTreeSet;

const TEMPLATE: &str = "application/x-scripted-bindings";
const SHA: [u8; 32] = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];

#[derive(Default)]
struct BoundSource {
    opens: RefCell<Vec<([u8; 32], u64)>>,
    error: Option<ErrorKind>,
    wrong_size: bool,
}

impl ContentSource for BoundSource {
    fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
        self.opens.borrow_mut().push((*sha256, size));
        if let Some(kind) = self.error {
            return Err(Error::new(kind, "planted content failure"));
        }
        if *sha256 != SHA || size != 3 {
            return Err(Error::new(
                ErrorKind::Content,
                "only bound digest is available",
            ));
        }
        if self.wrong_size {
            Ok(Content::Bytes(Vec::new()))
        } else {
            Ok(Content::Reader(Box::new(MemoryContent::new(
                b"abc".to_vec(),
            ))))
        }
    }
}

fn package(id: &str) -> BindingUse {
    BindingUse::new(
        id,
        "manifest-receipt",
        vec![
            BindingItem::new("rom", BindingKind::FirmwareRom, 3, SHA)
                .with_roles(vec!["system".into(), "other".into()]),
            BindingItem::new("disk", BindingKind::Medium, 3, SHA),
        ],
    )
}

fn compose(
    text: &str,
    uses: &[BindingUse],
    source: &dyn ContentSource,
) -> Result<PreparedConfig, Error> {
    let core: &dyn Core = &ScriptedBindingCore;
    core.compose(
        "scripted",
        ConfigDocument::new(TEMPLATE, text.as_bytes()),
        BindingFacts::new(uses, source),
    )
}

#[test]
fn bindings_default_to_unsupported_before_arguments_through_a_core_object() {
    let source = BoundSource::default();
    let core: &dyn Core = &BareCore;
    let invalid = ConfigDocument::new("unknown", &[0xff]);
    assert!(!PlatformDescription::new("empty", "Empty").content_bindings);
    assert_eq!(
        core.binding_slots("unknown", invalid).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        core.compose(
            "unknown",
            invalid,
            BindingFacts::new(&[BindingUse::new("", "", vec![])], &source)
        )
        .unwrap_err()
        .kind,
        ErrorKind::Unsupported
    );
    assert!(source.opens.borrow().is_empty());
}

#[test]
fn binding_slots_are_canonical_occurrences_with_document_constraints() {
    let text = format!("z pkg rom firmware system 3 {}\na pkg disk medium - - -\nb pkg rom firmware - - -\nc - system zero - 4096 -\n", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let core: &dyn Core = &ScriptedBindingCore;
    assert!(core.description().platforms[0].content_bindings);
    let query = |text: &str| {
        core.binding_slots("scripted", ConfigDocument::new(TEMPLATE, text.as_bytes()))
            .unwrap()
    };
    let slots = query(&text);
    let reversed = text.lines().rev().collect::<Vec<_>>().join("\n");
    assert_eq!(slots, query(&reversed));
    assert_eq!(
        slots
            .iter()
            .map(|slot| slot.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c", "z"]
    );
    assert_eq!(
        slots[0],
        BindingSlot::new("a", "pkg", "disk", BindingKind::Medium)
    );
    assert_eq!(slots[2], BindingSlot::zero_disk("c", "system", 4096));
    assert_eq!(
        slots[3],
        BindingSlot::new("z", "pkg", "rom", BindingKind::FirmwareRom)
            .with_role("system")
            .with_size(3)
            .with_sha256(SHA)
    );
}

#[test]
fn composition_rejects_inconsistent_facts_before_opening_bytes() {
    let source = BoundSource::default();
    let normal = "s pkg rom firmware system 3 -";
    let mut ambiguous = package("pkg");
    ambiguous.items.push(ambiguous.items[0].clone());
    let mut wrong_size = package("pkg");
    wrong_size.items[0].size = 4;
    let mut undeclared = package("pkg");
    undeclared.items[0].roles = vec!["video".into()];
    let mut wrong_kind = package("pkg");
    wrong_kind.items[0].kind = BindingKind::Medium;
    let cases = vec![
        (normal.to_owned(), vec![], "unbound use"),
        (
            normal.to_owned(),
            vec![package("pkg"), package("extra")],
            "unused use",
        ),
        (normal.to_owned(), vec![ambiguous], "ambiguous item id"),
        (normal.to_owned(), vec![wrong_size], "size conflict"),
        (
            format!("s pkg rom firmware system 3 {}", "00".repeat(32)),
            vec![package("pkg")],
            "digest conflict",
        ),
        (normal.to_owned(), vec![undeclared], "undeclared role"),
        (normal.to_owned(), vec![wrong_kind], "wrong kind"),
        (
            normal.to_owned(),
            vec![package("pkg"), package("pkg")],
            "duplicate use id",
        ),
        (
            "s pkg missing firmware system 3 -".into(),
            vec![package("pkg")],
            "missing item",
        ),
        (
            normal.to_owned(),
            vec![BindingUse::new("pkg", "", vec![])],
            "manifest digest",
        ),
        (
            format!("{normal} logical-path"),
            vec![package("pkg")],
            "path and binding conflict",
        ),
    ];
    for (text, uses, reason) in cases {
        let error = compose(&text, &uses, &source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Config, "{reason}");
        assert!(error.message.contains(reason), "{error}");
        assert!(!error.diagnostics[0].path.is_empty());
        assert!(source.opens.borrow().is_empty(), "{reason}");
    }
}

#[test]
fn roles_cannot_be_borrowed_from_another_package_or_item_with_the_same_digest() {
    let mut video = package("video");
    video.items[0].roles = vec!["video".into()];
    let source = BoundSource::default();
    let error = compose(
        "a system rom firmware system 3 -\nb video rom firmware system 3 -",
        &[package("system"), video],
        &source,
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Config);
    assert_eq!(error.diagnostics[0].path, "b");
    let mut package = package("pkg");
    package.items[0].roles = vec!["video".into()];
    package.items.push(
        BindingItem::new("other", BindingKind::FirmwareRom, 3, SHA)
            .with_roles(vec!["system".into()]),
    );
    assert!(
        compose("s pkg rom firmware system 3 -", &[package], &source)
            .unwrap_err()
            .message
            .contains("undeclared role")
    );
}

#[test]
fn prepared_config_is_deterministic_and_create_needs_only_bound_digests() {
    let source = BoundSource::default();
    let mut uses = vec![package("one"), package("two")];
    let text = "z one rom firmware system 3 -\na two disk medium - - -\nb one rom firmware other - -\nc - system zero - 4096 -";
    let result = compose(text, &uses, &source).unwrap();
    uses.reverse();
    for package in &mut uses {
        package.items.reverse();
        for item in &mut package.items {
            item.roles.reverse();
        }
    }
    let reordered = text.lines().rev().collect::<Vec<_>>().join("\n");
    assert_eq!(result, compose(&reordered, &uses, &source).unwrap());
    assert_eq!(
        result.validated,
        Validated::new(vec![ContentRef::new(SHA, 3, "bound content")])
    );
    let core: &dyn Core = &ScriptedBindingCore;
    check_prepared(core, "scripted", &result).unwrap();
    assert_eq!(
        core.validate("scripted", result.config.as_document())
            .unwrap(),
        result.validated
    );
    assert_eq!(
        core.binding_slots("scripted", result.config.as_document())
            .unwrap(),
        [BindingSlot::zero_disk("c", "system", 4096)]
    );
    source.opens.borrow_mut().clear();
    let mut machine: Box<dyn Machine> = core
        .create(&CreateRequest::new(
            "scripted",
            result.config.as_document(),
            &source,
            Rc::new(MemoryStorage::new()),
        ))
        .unwrap();
    assert_eq!(machine.status(), Status::Ready);
    machine.finish().unwrap();
    assert_eq!(
        source
            .opens
            .borrow()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(SHA, 3)])
    );
    let changed = PreparedConfig::new(result.config, Validated::new(vec![]));
    assert!(check_prepared(core, "scripted", &changed)
        .unwrap_err()
        .message
        .contains("content set mismatch"));
}

#[test]
fn zero_disks_need_no_package_or_content_and_empty_configs_compose() {
    let source = BoundSource {
        error: Some(ErrorKind::Content),
        ..BoundSource::default()
    };
    for text in ["s - system zero - 4096 -", ""] {
        let result = compose(text, &[], &source).unwrap();
        assert!(result.validated.content.is_empty());
        check_prepared(&ScriptedBindingCore, "scripted", &result).unwrap();
    }
    assert!(source.opens.borrow().is_empty());
}

#[test]
fn zero_disk_ids_survive_preparation_and_are_unique_within_the_document() {
    let core: &dyn Core = &ScriptedBindingCore;
    let source = BoundSource::default();
    let text = "a - system zero - 528482304 -\nb - data zero - 528482304 -";
    let slots = core
        .binding_slots("scripted", ConfigDocument::new(TEMPLATE, text.as_bytes()))
        .unwrap();
    assert_eq!(slots.len(), 2);
    for (slot, disk_id) in slots.iter().zip(["system", "data"]) {
        assert_eq!(slot.use_id, None);
        assert_eq!(slot.item_id.as_deref(), Some(disk_id));
        assert_eq!(slot.size, Some(528482304));
    }
    let prepared = compose(text, &[], &source).unwrap();
    assert_eq!(
        core.binding_slots("scripted", prepared.config.as_document())
            .unwrap(),
        slots
    );
    for invalid in [
        "a - system zero - 4096 -\nb - system zero - 4096 -",
        "a - - zero - 4096 -",
    ] {
        assert_eq!(
            compose(invalid, &[], &source).unwrap_err().kind,
            ErrorKind::Config
        );
    }
    assert!(source.opens.borrow().is_empty());
}

#[test]
fn composition_rejects_empty_role_names_before_opening_content() {
    let source = BoundSource::default();
    let mut package = package("pkg");
    package.items[0].roles.push(String::new());
    let error = compose("s pkg rom firmware system 3 -", &[package], &source).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Config);
    assert!(error.message.contains("empty role"));
    assert!(source.opens.borrow().is_empty());
}

#[test]
fn composition_and_create_reject_unusable_verified_content_as_unsupported() {
    struct VerifiedSource {
        bytes: Vec<u8>,
        sha256: [u8; 32],
    }
    impl ContentSource for VerifiedSource {
        fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
            assert_eq!(*sha256, self.sha256);
            assert_eq!(size, self.bytes.len() as u64);
            Ok(Content::Reader(Box::new(MemoryContent::new(
                self.bytes.clone(),
            ))))
        }
    }
    let cases = [
        (
            vec![0xff],
            [
                168, 16, 10, 230, 170, 25, 64, 208, 182, 99, 187, 49, 205, 70, 97, 66, 235, 189,
                189, 81, 135, 19, 27, 146, 217, 56, 24, 152, 120, 50, 235, 137,
            ],
        ),
        (
            vec![],
            [
                227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36, 39,
                174, 65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85,
            ],
        ),
    ];
    let core: &dyn Core = &ScriptedBindingCore;
    for (bytes, sha256) in cases {
        let size = bytes.len() as u64;
        let source = VerifiedSource { bytes, sha256 };
        let uses = [BindingUse::new(
            "pkg",
            "manifest-receipt",
            vec![BindingItem::new("cart", BindingKind::Medium, size, sha256)],
        )];
        let error = compose("s pkg cart medium - - -", &uses, &source).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unsupported);
        let digest: String = sha256.iter().map(|byte| format!("{byte:02x}")).collect();
        let document = format!("s medium - {size} {digest}\n");
        let config = ConfigDocument::new("application/x-scripted-prepared", document.as_bytes());
        assert!(core.validate("scripted", config).is_ok());
        let error = core
            .create(&CreateRequest::new(
                "scripted",
                config,
                &source,
                Rc::new(MemoryStorage::new()),
            ))
            .err()
            .expect("unusable content must fail creation");
        assert_eq!(error.kind, ErrorKind::Unsupported);
    }
}

#[test]
fn composition_preserves_content_and_io_errors_and_checks_receipt_sizes() {
    for kind in [ErrorKind::Content, ErrorKind::Storage] {
        let source = BoundSource {
            error: Some(kind),
            ..BoundSource::default()
        };
        assert_eq!(
            compose("s pkg rom firmware system 3 -", &[package("pkg")], &source)
                .unwrap_err()
                .kind,
            kind
        );
    }
    let source = BoundSource {
        wrong_size: true,
        ..BoundSource::default()
    };
    assert_eq!(
        compose("s pkg rom firmware system 3 -", &[package("pkg")], &source)
            .unwrap_err()
            .kind,
        ErrorKind::Content
    );
    assert_eq!(
        ScriptedBindingCore
            .binding_slots("absent", ConfigDocument::new(TEMPLATE, &[0xff]))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
