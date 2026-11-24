// SPDX-License-Identifier: MIT OR Apache-2.0
//! A reference binding core using a deliberately small document syntax.

use super::ScriptedMachine;
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

const TEMPLATE: &str = "application/x-scripted-bindings";
const PREPARED: &str = "application/x-scripted-prepared";
const DESCRIPTION: CoreDescription = CoreDescription::new(
    ContractVersion::new(0, 6),
    "scripted-bindings",
    "Scripted bindings",
    "1.0.0",
)
.with_platforms(&[PlatformDescription::new("scripted", "Scripted")
    .with_config_types(&[TEMPLATE, PREPARED])
    .with_content_bindings(true)]);

/// Reference core for front-end binding tests, creating ScriptedMachine.
/// Platform `scripted` accepts UTF-8 line documents:
///
/// - `application/x-scripted-bindings`: `slot use item kind role size digest`.
/// - `application/x-scripted-prepared`: `slot kind role size digest`.
///   For `zero`, the role field instead holds the document-declared disk id.
///
/// Fields are whitespace separated. Kinds are `firmware`, `medium`, `zero`;
/// optional role, size and digest use `-`. Digests are 64 lowercase hex digits.
/// Zero disks require size and a document-unique item (disk) id, with `-` for
/// use/role/digest in templates. An eighth template
/// field models a forbidden path-plus-binding conflict. All sizes are bytes.
/// Lines may be reordered; output and slots are sorted by slot id. Empty
/// documents describe no content. This syntax is solely a testing fixture.
/// Composition and creation inspect medium bytes: an empty medium is damaged,
/// and a leading 0xff models unsupported content. Both return Unsupported.
pub struct ScriptedBindingCore;

fn rejected(path: &str, reason: &str) -> Error {
    Error::new(ErrorKind::Config, format!("{path}: {reason}"))
        .with_diagnostics(vec![Diagnostic::new(path, reason)])
}

fn digest(value: &str) -> Result<Option<[u8; 32]>, Error> {
    if value == "-" {
        return Ok(None);
    }
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(rejected("digest", "expected lowercase SHA-256"));
    }
    let mut result = [0; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| rejected("digest", "invalid SHA-256"))?;
    }
    Ok(Some(result))
}

fn hex(value: &[u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn kind_name(kind: BindingKind) -> &'static str {
    match kind {
        BindingKind::FirmwareRom => "firmware",
        BindingKind::Medium => "medium",
        BindingKind::ZeroDisk => "zero",
    }
}

fn parse(platform: &str, config: ConfigDocument<'_>) -> Result<Vec<BindingSlot>, Error> {
    if platform != "scripted" || ![TEMPLATE, PREPARED].contains(&config.media_type) {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "unknown platform or config type",
        ));
    }
    let template = config.media_type == TEMPLATE;
    let text =
        std::str::from_utf8(config.bytes).map_err(|_| rejected("config", "expected UTF-8"))?;
    let mut slots = BTreeMap::new();
    let mut disk_ids = BTreeSet::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        let id = fields[0];
        if template && fields.len() == 8 {
            return Err(rejected(id, "path and binding conflict"));
        }
        if fields.len() != if template { 7 } else { 5 } {
            return Err(rejected(id, "invalid binding fields"));
        }
        let offset = if template { 2 } else { 0 };
        let kind = match fields[1 + offset] {
            "firmware" => BindingKind::FirmwareRom,
            "medium" => BindingKind::Medium,
            "zero" => BindingKind::ZeroDisk,
            _ => return Err(rejected(id, "unknown binding kind")),
        };
        let role = (fields[2 + offset] != "-" && (template || kind != BindingKind::ZeroDisk))
            .then(|| fields[2 + offset].to_owned());
        let size = if fields[3 + offset] == "-" {
            None
        } else {
            Some(
                fields[3 + offset]
                    .parse::<u64>()
                    .map_err(|_| rejected(id, "invalid size"))?,
            )
        };
        let sha256 = digest(fields[4 + offset])?;
        let mut slot = if kind == BindingKind::ZeroDisk {
            if role.is_some() || sha256.is_some() || (template && fields[1] != "-") {
                return Err(rejected(id, "zero disk has no package, role or digest"));
            }
            let disk_id = fields[2];
            if disk_id == "-" || !disk_ids.insert(disk_id) {
                return Err(rejected(id, "zero disk needs a unique disk id"));
            }
            BindingSlot::zero_disk(
                id,
                disk_id,
                size.ok_or_else(|| rejected(id, "zero disk needs size"))?,
            )
        } else {
            if template && (fields[1] == "-" || fields[2] == "-") {
                return Err(rejected(id, "binding needs use and item ids"));
            }
            if !template && (size.is_none() || sha256.is_none()) {
                return Err(rejected(id, "prepared content needs size and digest"));
            }
            BindingSlot::new(
                id,
                if template { fields[1] } else { "" },
                if template { fields[2] } else { "" },
                kind,
            )
        };
        slot.role = role;
        slot.size = size;
        slot.sha256 = sha256;
        if !template && kind != BindingKind::ZeroDisk {
            slot.use_id = None;
            slot.item_id = None;
        }
        if slots.insert(id.to_owned(), slot).is_some() {
            return Err(rejected(id, "duplicate slot id"));
        }
    }
    Ok(slots.into_values().collect())
}

fn validation(slots: &[BindingSlot]) -> Validated {
    let identities: BTreeSet<_> = slots
        .iter()
        .filter_map(|slot| slot.sha256.zip(slot.size))
        .collect();
    Validated::new(
        identities
            .into_iter()
            .map(|(sha256, size)| ContentRef::new(sha256, size, "bound content"))
            .collect(),
    )
}

fn open_required(
    source: &dyn ContentSource,
    validated: &Validated,
    slots: &[BindingSlot],
) -> Result<(), Error> {
    for item in &validated.content {
        let bytes = source.open(&item.sha256, item.size)?;
        if bytes.size() != item.size {
            return Err(Error::new(
                ErrorKind::Content,
                "bound content receipt size mismatch",
            ));
        }
        if slots.iter().any(|slot| {
            slot.kind == BindingKind::Medium
                && slot.sha256 == Some(item.sha256)
                && slot.size == Some(item.size)
        }) {
            if item.size == 0 {
                return Err(Error::new(ErrorKind::Unsupported, "damaged empty medium"));
            }
            let mut header = [0];
            bytes.read_at(0, &mut header)?;
            if header[0] == 0xff {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "unsupported medium marker",
                ));
            }
        }
    }
    Ok(())
}

impl Core for ScriptedBindingCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn binding_slots(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
    ) -> Result<Vec<BindingSlot>, Error> {
        let slots = parse(platform, config)?;
        Ok(slots
            .into_iter()
            .filter(|slot| slot.use_id.is_some() || slot.kind == BindingKind::ZeroDisk)
            .collect())
    }

    fn compose(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
        facts: BindingFacts<'_>,
    ) -> Result<PreparedConfig, Error> {
        let mut slots = parse(platform, config)?;
        let needed: BTreeSet<_> = slots
            .iter()
            .filter_map(|slot| slot.use_id.as_deref())
            .collect();
        let mut uses = BTreeMap::new();
        for package in facts.uses {
            if package.id.is_empty() || package.manifest_digest.is_empty() {
                return Err(rejected(&package.id, "use needs an id and manifest digest"));
            }
            if uses.insert(package.id.as_str(), package).is_some() {
                return Err(rejected(&package.id, "duplicate use id"));
            }
            if !needed.contains(package.id.as_str()) {
                return Err(rejected(&package.id, "unused use"));
            }
            let mut ids = BTreeSet::new();
            for item in &package.items {
                if item.id.is_empty() {
                    return Err(rejected(&package.id, "empty item id"));
                }
                if item.roles.iter().any(String::is_empty) {
                    return Err(rejected(
                        &package.id,
                        &format!("empty role for item: {}", item.id),
                    ));
                }
                if !ids.insert(&item.id) {
                    return Err(rejected(
                        &package.id,
                        &format!("ambiguous item id: {}", item.id),
                    ));
                }
                if item.kind == BindingKind::ZeroDisk {
                    return Err(rejected(
                        &package.id,
                        "wrong kind: package cannot declare zero disk",
                    ));
                }
            }
        }
        for slot in &mut slots {
            let Some(use_id) = &slot.use_id else { continue };
            let package = uses
                .get(use_id.as_str())
                .ok_or_else(|| rejected(&slot.id, &format!("unbound use: {use_id}")))?;
            let item_id = slot.item_id.as_ref().expect("parsed package reference");
            let item = package
                .items
                .iter()
                .find(|item| &item.id == item_id)
                .ok_or_else(|| rejected(&slot.id, &format!("missing item: {use_id}/{item_id}")))?;
            if slot.kind != item.kind {
                return Err(rejected(&slot.id, "wrong kind for bound item"));
            }
            if slot.size.is_some_and(|size| size != item.size) {
                return Err(rejected(&slot.id, "size conflict between config and facts"));
            }
            if slot.sha256.is_some_and(|sha256| sha256 != item.sha256) {
                return Err(rejected(
                    &slot.id,
                    "digest conflict between config and facts",
                ));
            }
            if slot
                .role
                .as_ref()
                .is_some_and(|role| !item.roles.contains(role))
            {
                return Err(rejected(&slot.id, "undeclared role for bound package item"));
            }
            slot.size = Some(item.size);
            slot.sha256 = Some(item.sha256);
        }
        let mut document = String::new();
        for slot in &slots {
            document.push_str(&format!(
                "{} {} {} {} {}\n",
                slot.id,
                kind_name(slot.kind),
                if slot.kind == BindingKind::ZeroDisk {
                    slot.item_id.as_deref().expect("parsed disk id")
                } else {
                    slot.role.as_deref().unwrap_or("-")
                },
                slot.size.expect("resolved size"),
                slot.sha256.as_ref().map(hex).unwrap_or_else(|| "-".into())
            ));
        }
        let config = ConfigDocumentBuf::new(PREPARED, document.into_bytes());
        let validated = self.validate(platform, config.as_document())?;
        open_required(facts.content, &validated, &slots)?;
        Ok(PreparedConfig::new(config, validated))
    }

    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        let slots = parse(platform, config)?;
        if slots.iter().any(|slot| slot.use_id.is_some()) {
            return Err(rejected("config", "unresolved bindings need composition"));
        }
        Ok(validation(&slots))
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        let validated = self.validate(request.platform, request.config)?;
        if !request.options.is_empty() {
            return Err(Error::new(ErrorKind::Usage, "unknown run option"));
        }
        open_required(
            request.content,
            &validated,
            &parse(request.platform, request.config)?,
        )?;
        Ok(Box::new(ScriptedMachine::new(request.storage.clone())))
    }
}

/// Check a core's prepared result with its config-only validator. Identity
/// lists use set semantics; document bytes and labels remain core-owned.
/// This helper performs no content or storage I/O and creates no machine.
pub fn check_prepared(
    core: &dyn Core,
    platform: &str,
    prepared: &PreparedConfig,
) -> Result<(), Error> {
    let validated = core.validate(platform, prepared.config.as_document())?;
    let identities = |validated: &Validated| -> BTreeSet<_> {
        validated
            .content
            .iter()
            .map(|item| (item.sha256, item.size))
            .collect()
    };
    if identities(&validated) != identities(&prepared.validated) {
        return Err(rejected("prepared", "validation content set mismatch"));
    }
    if core
        .binding_slots(platform, prepared.config.as_document())?
        .iter()
        .any(|slot| slot.kind != BindingKind::ZeroDisk)
    {
        return Err(rejected("prepared", "unresolved package binding"));
    }
    Ok(())
}
