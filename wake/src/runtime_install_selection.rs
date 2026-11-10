//! Same-state selection proposals over the existing `current.json`.
//!
//! Like the ordinary-worker release store, current and its qualified working
//! predecessor share one atomic document. The legacy flat fields remain intact
//! for old readers. This module performs no I/O and grants no installation or
//! rollback authority: a future retained-owner transaction must recheck its CAS,
//! invoke an independently admitted verifier, and use the existing atomic writer.

use crate::Result;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const INVALID: &str = "HOST_INSTALL_SELECTION_INVALID";
const MAX_SELECTION_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PointerV1 {
    schema_version: u32,
    directory: String,
    host_sha256: String,
    binagotchy_sha256: String,
    adapter_sha256: String,
    wake_bridge_sha256: String,
}

impl PointerV1 {
    fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self).map_err(|_| INVALID)?;
        if self.schema_version != 1 {
            return Err(INVALID.into());
        }
        super::install_id(&value)?;
        for field in [
            "hostSha256",
            "binagotchySha256",
            "adapterSha256",
            "wakeBridgeSha256",
        ] {
            super::pointer_hash(&value, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseV2 {
    pointer: PointerV1,
    qualification_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectionV2 {
    schema_version: u32,
    generation: u64,
    current: ReleaseV2,
    previous: Option<ReleaseV2>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurrentDocument {
    schema_version: u32,
    directory: String,
    host_sha256: String,
    binagotchy_sha256: String,
    adapter_sha256: String,
    wake_bridge_sha256: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_selection"
    )]
    selection: Option<SelectionV2>,
}

fn present_selection<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<SelectionV2>, D::Error> {
    // An explicitly null/corrupt extension cannot fall back to legacy mode.
    SelectionV2::deserialize(deserializer).map(Some)
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl CurrentDocument {
    fn pointer(&self) -> PointerV1 {
        PointerV1 {
            schema_version: self.schema_version,
            directory: self.directory.clone(),
            host_sha256: self.host_sha256.clone(),
            binagotchy_sha256: self.binagotchy_sha256.clone(),
            adapter_sha256: self.adapter_sha256.clone(),
            wake_bridge_sha256: self.wake_bridge_sha256.clone(),
        }
    }

    fn from_selection(selection: SelectionV2) -> Self {
        let p = selection.current.pointer.clone();
        Self {
            schema_version: p.schema_version,
            directory: p.directory,
            host_sha256: p.host_sha256,
            binagotchy_sha256: p.binagotchy_sha256,
            adapter_sha256: p.adapter_sha256,
            wake_bridge_sha256: p.wake_bridge_sha256,
            selection: Some(selection),
        }
    }

    fn validate(&self) -> Result<()> {
        self.pointer().validate()?;
        if let Some(s) = &self.selection {
            if s.schema_version != 2 || s.generation == 0 || s.current.pointer != self.pointer() {
                return Err(INVALID.into());
            }
            for r in std::iter::once(&s.current).chain(s.previous.iter()) {
                r.pointer.validate()?;
                if !hash(&r.qualification_sha256) {
                    return Err(INVALID.into());
                }
            }
            if s.previous
                .as_ref()
                .is_some_and(|p| p.pointer.directory == s.current.pointer.directory)
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
}

fn parse(bytes: &[u8]) -> Result<CurrentDocument> {
    if bytes.is_empty() || bytes.len() > MAX_SELECTION_BYTES {
        return Err(INVALID.into());
    }
    let value: CurrentDocument = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    value.validate()?;
    Ok(value)
}

fn check_cas(bytes: &[u8], expected_sha256: &str) -> Result<()> {
    if !hash(expected_sha256) || format!("{:x}", Sha256::digest(bytes)) != expected_sha256 {
        return Err("HOST_INSTALL_SELECTION_CAS_REFUSED".into());
    }
    Ok(())
}

fn encode(selection: SelectionV2) -> Result<Vec<u8>> {
    let value = CurrentDocument::from_selection(selection);
    value.validate()?;
    let bytes = serde_json::to_vec(&value).map_err(|_| INVALID)?;
    parse(&bytes)?;
    Ok(bytes)
}

/// Bounded structural readback only. Qualification references are untrusted
/// until the supplied release verifier establishes their exact retained facts.
pub fn inspect_install_selection(bytes: &[u8]) -> Result<Value> {
    let document = parse(bytes)?;
    Ok(serde_json::json!({
        "current":document.pointer(), "selection":document.selection,
        "qualificationVerified":false, "installAuthorized":false,
    }))
}

/// Pure proposal for one same-root pointer replacement. `verify` must establish
/// artifact and reviewed-release evidence; `working=true` additionally requires
/// independently retained working evidence. Parsing a hash or old selection is
/// never that proof. No production verifier or writer is enabled here.
pub fn propose_install_selection(
    before: &[u8],
    expected_sha256: &str,
    candidate: &Value,
    candidate_qualification: &str,
    legacy_current_qualification: Option<&str>,
    verify: &dyn Fn(&Value, &str, bool) -> Result<()>,
) -> Result<Vec<u8>> {
    check_cas(before, expected_sha256)?;
    let prior = parse(before)?;
    let prior_pointer = prior.pointer();
    let pointer: PointerV1 = serde_json::from_value(candidate.clone()).map_err(|_| INVALID)?;
    pointer.validate()?;
    if pointer.directory == prior.directory || !hash(candidate_qualification) {
        return Err(INVALID.into());
    }
    let verify_release = |r: &ReleaseV2, working| {
        verify(
            &serde_json::to_value(&r.pointer).map_err(|_| INVALID)?,
            &r.qualification_sha256,
            working,
        )
    };
    let (generation, previous) = match prior.selection {
        Some(s) => {
            if legacy_current_qualification.is_some() {
                return Err(INVALID.into());
            }
            // A damaged/unqualified current must not strand an intact,
            // independently qualified working predecessor. Never substitute
            // mere previous selection for the verifier's retained evidence.
            let previous = if verify_release(&s.current, true).is_ok() {
                s.current
            } else {
                let previous = s
                    .previous
                    .ok_or("HOST_INSTALL_PREVIOUS_QUALIFICATION_REQUIRED")?;
                verify_release(&previous, true)?;
                previous
            };
            (s.generation, previous)
        }
        None => {
            let qualification = legacy_current_qualification
                .filter(|q| hash(q))
                .ok_or("HOST_INSTALL_PREVIOUS_QUALIFICATION_REQUIRED")?;
            let previous = ReleaseV2 {
                pointer: prior_pointer,
                qualification_sha256: qualification.into(),
            };
            verify_release(&previous, true)?;
            (0, previous)
        }
    };
    verify(candidate, candidate_qualification, false)?;
    encode(SelectionV2 {
        schema_version: 2,
        generation: generation.checked_add(1).ok_or(INVALID)?,
        current: ReleaseV2 {
            pointer,
            qualification_sha256: candidate_qualification.into(),
        },
        previous: Some(previous),
    })
}

/// Propose recovery from an intact qualified predecessor even when current
/// artifact bytes are damaged. Current pointer/qualification metadata must
/// still parse; malformed selection or missing/unqualified previous refuses.
pub fn propose_install_selection_rollback(
    before: &[u8],
    expected_sha256: &str,
    verify: &dyn Fn(&Value, &str, bool) -> Result<()>,
) -> Result<Vec<u8>> {
    check_cas(before, expected_sha256)?;
    let selection = parse(before)?
        .selection
        .ok_or("HOST_INSTALL_PREVIOUS_QUALIFICATION_REQUIRED")?;
    let previous = selection
        .previous
        .ok_or("HOST_INSTALL_PREVIOUS_QUALIFICATION_REQUIRED")?;
    verify(
        &serde_json::to_value(&previous.pointer).map_err(|_| INVALID)?,
        &previous.qualification_sha256,
        true,
    )?;
    encode(SelectionV2 {
        schema_version: 2,
        generation: selection.generation.checked_add(1).ok_or(INVALID)?,
        current: previous,
        previous: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(version: u8) -> Value {
        serde_json::json!({"schemaVersion":1,"directory":format!("version-{version}"),
            "hostSha256":format!("{version:064x}"),"binagotchySha256":"b".repeat(64),
            "adapterSha256":"c".repeat(64),"wakeBridgeSha256":"d".repeat(64)})
    }
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }
    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn verified(pointer: &Value, qualification: &str, working: bool) -> Result<()> {
        let expected = match pointer["directory"].as_str() {
            Some("version-1") => "1".repeat(64),
            Some("version-2") if !working => "2".repeat(64),
            _ => return Err("FIXTURE_RELEASE_NOT_QUALIFIED".into()),
        };
        if qualification != expected {
            return Err("FIXTURE_QUALIFICATION_MISMATCH".into());
        }
        Ok(())
    }
    fn selected() -> Vec<u8> {
        let before = bytes(&pointer(1));
        propose_install_selection(
            &before,
            &digest(&before),
            &pointer(2),
            &"2".repeat(64),
            Some(&"1".repeat(64)),
            &verified,
        )
        .unwrap()
    }

    #[test]
    fn flat_extension_uses_unchanged_attested_legacy_readers() {
        let before = bytes(&pointer(1));
        assert!(inspect_install_selection(&before).unwrap()["selection"].is_null());
        let after: Value = serde_json::from_slice(&selected()).unwrap();
        // These exact functions are unchanged from serving source 9ea9e7be.
        assert_eq!(super::super::install_id(&after).unwrap(), "version-2");
        for field in [
            "hostSha256",
            "binagotchySha256",
            "adapterSha256",
            "wakeBridgeSha256",
        ] {
            assert_eq!(
                super::super::pointer_hash(&after, field).unwrap(),
                pointer(2)[field].as_str().unwrap()
            );
        }
        assert_eq!(after["schemaVersion"], 1);
        assert_eq!(after["selection"]["schemaVersion"], 2);
        assert_eq!(after["selection"]["previous"]["pointer"], pointer(1));
    }

    #[test]
    fn stale_cas_or_unqualified_old_selection_cannot_create_a_plan() {
        let before = bytes(&pointer(1));
        assert!(
            propose_install_selection(
                &before,
                &"0".repeat(64),
                &pointer(2),
                &"2".repeat(64),
                Some(&"1".repeat(64)),
                &verified
            )
            .is_err()
        );
        assert!(
            propose_install_selection(
                &before,
                &digest(&before),
                &pointer(2),
                &"2".repeat(64),
                None,
                &verified
            )
            .is_err()
        );
        assert!(
            propose_install_selection(
                &before,
                &digest(&before),
                &pointer(2),
                &"2".repeat(64),
                Some(&"f".repeat(64)),
                &verified
            )
            .is_err()
        );
        assert_eq!(before, bytes(&pointer(1)));
    }

    #[test]
    fn corrupt_current_artifacts_can_fall_back_only_to_verified_working_previous() {
        let before = selected();
        let verifier = |p: &Value, q: &str, w: bool| {
            assert_ne!(
                p["directory"], "version-2",
                "damaged current artifacts must not block predecessor recovery"
            );
            assert!(w);
            verified(p, q, w)
        };
        let after =
            propose_install_selection_rollback(&before, &digest(&before), &verifier).unwrap();
        let value: Value = serde_json::from_slice(&after).unwrap();
        assert_eq!(value["directory"], "version-1");
        assert_eq!(value["selection"]["generation"], 2);
        assert!(value["selection"]["previous"].is_null());
        assert!(
            propose_install_selection_rollback(&before, &digest(&before), &|_, _, _| Err(
                "DAMAGED_PREVIOUS".into()
            ))
            .is_err()
        );
    }

    #[test]
    fn malformed_or_divergent_extension_never_falls_back_to_legacy() {
        let value: Value = serde_json::from_slice(&selected()).unwrap();
        for changed in [
            {
                let mut v = value.clone();
                v["selection"] = Value::Null;
                v
            },
            {
                let mut v = value.clone();
                v["selection"]["current"]["pointer"]["directory"] = "other".into();
                v
            },
            {
                let mut v = value.clone();
                v["selection"]["generation"] = 0.into();
                v
            },
            {
                let mut v = value.clone();
                v["selection"]["unknown"] = true.into();
                v
            },
            {
                let mut v = value.clone();
                v["selection"]["previous"]["qualificationSha256"] = "bad".into();
                v
            },
        ] {
            assert!(inspect_install_selection(&bytes(&changed)).is_err());
        }
        let valid = String::from_utf8(selected()).unwrap();
        let duplicate = format!("{{\"schemaVersion\":1,{}", &valid[1..]);
        assert!(inspect_install_selection(duplicate.as_bytes()).is_err());
        let duplicate_nested =
            valid.replacen("\"generation\":1", "\"generation\":1,\"generation\":1", 1);
        assert!(inspect_install_selection(duplicate_nested.as_bytes()).is_err());
        assert!(inspect_install_selection(&selected()[..30]).is_err());
    }

    #[test]
    fn upgrading_damaged_current_retains_verified_previous_and_rejects_invalid_fallback() {
        let before = selected();
        let verifier = |p: &Value, q: &str, working: bool| {
            if p["directory"] == "version-3" && !working && q == "3".repeat(64) {
                Ok(())
            } else {
                verified(p, q, working)
            }
        };
        let next = propose_install_selection(
            &before,
            &digest(&before),
            &pointer(3),
            &"3".repeat(64),
            None,
            &verifier,
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&next).unwrap();
        assert_eq!(value["directory"], "version-3");
        assert_eq!(value["selection"]["previous"]["pointer"], pointer(1));
        let refuse_fallback = |p: &Value, q: &str, working: bool| {
            if working {
                Err("NO_WORKING_RELEASE".into())
            } else {
                verifier(p, q, working)
            }
        };
        assert!(
            propose_install_selection(
                &before,
                &digest(&before),
                &pointer(3),
                &"3".repeat(64),
                None,
                &refuse_fallback
            )
            .is_err()
        );
    }

    #[test]
    fn generation_overflow_or_stale_full_document_never_yields_selection() {
        let mut value: Value = serde_json::from_slice(&selected()).unwrap();
        value["selection"]["generation"] = u64::MAX.into();
        let before = bytes(&value);
        assert!(propose_install_selection_rollback(&before, &digest(&before), &verified).is_err());
        assert!(
            propose_install_selection(
                &before,
                &digest(&before),
                &pointer(3),
                &"3".repeat(64),
                None,
                &|_, _, _| Ok(())
            )
            .is_err()
        );
        value["selection"]["generation"] = true.into();
        assert!(inspect_install_selection(&bytes(&value)).is_err());
        let before = selected();
        let mut changed: Value = serde_json::from_slice(&before).unwrap();
        changed["selection"]["previous"]["qualificationSha256"] = "f".repeat(64).into();
        assert!(
            propose_install_selection_rollback(&bytes(&changed), &digest(&before), &verified)
                .is_err()
        );
    }

    #[test]
    fn interruption_and_planning_leave_state_bindings_and_profile_unchanged() {
        let (store, _lease) = super::super::scheduler::tests::fixture("same-state-selection");
        let root = store.root();
        let current = root.join("current.json");
        crate::store::atomic(&current, &pointer(1)).unwrap();
        let before = std::fs::read(&current).unwrap();
        let profile = root.join("browser-profile");
        std::fs::create_dir(&profile).unwrap();
        std::fs::write(
            profile.join("opaque-session"),
            b"unchanged authentication state",
        )
        .unwrap();
        let journal = root.join("unknown-task.json");
        std::fs::write(&journal, b"UNKNOWN remains UNKNOWN").unwrap();
        let protected = [
            root.join("config.json"),
            profile.join("opaque-session"),
            journal,
        ];
        let snapshots: Vec<_> = protected
            .iter()
            .map(std::fs::read)
            .collect::<std::io::Result<_>>()
            .unwrap();
        let prepared = propose_install_selection(
            &before,
            &digest(&before),
            &pointer(2),
            &"2".repeat(64),
            Some(&"1".repeat(64)),
            &verified,
        )
        .unwrap();
        // Interrupted staging is inert: an abandoned truncated temp document
        // cannot replace the existing current selection or become a fallback.
        std::fs::write(root.join("current.interrupted.tmp"), &prepared[..20]).unwrap();
        assert_eq!(std::fs::read(&current).unwrap(), before);
        let value: Value = serde_json::from_slice(&prepared).unwrap();
        crate::store::atomic(&current, &value).unwrap();
        let committed = std::fs::read(&current).unwrap();
        assert_eq!(
            inspect_install_selection(&committed).unwrap()["current"]["directory"],
            "version-2"
        );
        assert!(
            propose_install_selection_rollback(&committed, &digest(&before), &verified).is_err()
        );
        let rollback =
            propose_install_selection_rollback(&committed, &digest(&committed), &verified).unwrap();
        crate::store::atomic(
            &current,
            &serde_json::from_slice::<Value>(&rollback).unwrap(),
        )
        .unwrap();
        assert_eq!(
            inspect_install_selection(&std::fs::read(&current).unwrap()).unwrap()["current"]["directory"],
            "version-1"
        );
        for (path, snapshot) in protected.iter().zip(snapshots) {
            assert_eq!(std::fs::read(path).unwrap(), snapshot);
        }
    }
}
