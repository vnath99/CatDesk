//! Deterministic signing fixtures only; no real key, SCM or protected object.
use super::*;
use ed25519_dalek::{Signer, SigningKey};

fn fixture() -> (BootstrapManifestV1, SigningKey) {
    let key = SigningKey::from_bytes(&[31; 32]);
    (
        BootstrapManifestV1 {
            schema_version: 1,
            purpose: PURPOSE.into(),
            bootstrap_id: "bootstrap-fixture".into(),
            issued_utc_seconds: 900,
            expires_utc_seconds: 1100,
            source_commit: "1".repeat(40),
            installer: BootstrapInstallerV1 {
                byte_length: 100,
                sha256: "2".repeat(64),
            },
            broker_artifact: BootstrapArtifactV1 {
                path: r"C:\fixture\broker.exe".into(),
                byte_length: 200,
                sha256: "3".repeat(64),
            },
            independent_review: BootstrapArtifactV1 {
                path: r"C:\fixture\review.json".into(),
                byte_length: 300,
                sha256: "4".repeat(64),
            },
            approval_public_key_hex: key
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            approval_public_key_sha256: digest(&key.verifying_key().to_bytes()),
            operator_sid: "S-1-5-21-1-2-3-1001".into(),
            reviewer_identity: "bootstrap-reviewer".into(),
            state_root: BootstrapStateV1 {
                path: r"C:\fixture\existing-state".into(),
                identity: FullFileIdentityV2 {
                    volume_serial: 1,
                    file_id_hex: "5".repeat(32),
                },
                current_sha256: "6".repeat(64),
                control_sha256: "7".repeat(64),
                config_sha256: "8".repeat(64),
            },
            allow_reviewed_reconstruction: false,
            allow_operator_service_start: true,
            allow_local_user_updates: None,
        },
        key,
    )
}
fn envelope(m: BootstrapManifestV1, key: &SigningKey) -> Vec<u8> {
    let mut bytes = DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(&m).unwrap());
    let signature_hex = key
        .sign(&bytes)
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    serde_json::to_vec(&BootstrapEnvelopeV1 {
        payload: m,
        signature_hex,
    })
    .unwrap()
}
#[test]
fn first_bootstrap_signature_is_purpose_and_external_fingerprint_bound() {
    let (m, key) = fixture();
    let fp = m.approval_public_key_sha256.clone();
    let raw = envelope(m, &key);
    let approved = verify_bootstrap(&raw, &digest(&raw), &fp, 1000).unwrap();
    let value = readback(&approved);
    assert_eq!(value["releaseApprovalCreated"], false);
    assert_eq!(value["selectionChanged"], false);
    assert!(verify_bootstrap(&raw, &"a".repeat(64), &fp, 1000).is_err());
    assert!(verify_bootstrap(&raw, &digest(&raw), &"a".repeat(64), 1000).is_err());
    assert!(verify_bootstrap(&raw, &digest(&raw), &fp, 1200).is_err());
    let mut changed = raw.clone();
    let last = changed.len() - 4;
    changed[last] ^= 1;
    assert!(verify_bootstrap(&changed, &digest(&changed), &fp, 1000).is_err());
}

#[test]
fn first_bootstrap_local_update_delegation_is_explicit_and_signature_bound() {
    let (mut manifest, key) = fixture();
    let fingerprint = manifest.approval_public_key_sha256.clone();
    let legacy = envelope(manifest.clone(), &key);
    assert!(
        !String::from_utf8(legacy.clone())
            .unwrap()
            .contains("allowLocalUserUpdates")
    );
    assert_eq!(
        verify_bootstrap(&legacy, &digest(&legacy), &fingerprint, 1000)
            .unwrap()
            .manifest
            .allow_local_user_updates,
        None
    );
    manifest.allow_local_user_updates = Some(true);
    let enrolled = envelope(manifest, &key);
    assert_eq!(
        verify_bootstrap(&enrolled, &digest(&enrolled), &fingerprint, 1000)
            .unwrap()
            .manifest
            .allow_local_user_updates,
        Some(true)
    );
    let text = String::from_utf8(enrolled).unwrap().replace(
        "\"allowLocalUserUpdates\":true",
        "\"allowLocalUserUpdates\":false",
    );
    assert!(
        verify_bootstrap(
            text.as_bytes(),
            &digest(text.as_bytes()),
            &fingerprint,
            1000
        )
        .is_err()
    );
}
#[test]
fn first_bootstrap_closed_scope_refuses_paths_flags_zero_identity_and_release_purposes() {
    let (original, key) = fixture();
    for case in 0..9 {
        let mut m = original.clone();
        match case {
            0 => m.purpose = "SELECT_SAME_STATE_RELEASE".into(),
            1 => m.allow_operator_service_start = false,
            2 => m.state_root.path = r"\\server\share".into(),
            3 => m.broker_artifact.path = r"C:\fixture\..\broker.exe".into(),
            4 => m.state_root.identity.file_id_hex = "0".repeat(32),
            5 => m.broker_artifact.byte_length = 0,
            6 => m.installer.byte_length = 80 * 1024 * 1024 + 1,
            7 => m.expires_utc_seconds = 90000,
            8 => m.source_commit = "not-commit".into(),
            _ => unreachable!(),
        }
        let fp = m.approval_public_key_sha256.clone();
        let raw = envelope(m, &key);
        assert!(
            verify_bootstrap(&raw, &digest(&raw), &fp, 1000).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn first_bootstrap_encoding_refuses_unknown_duplicate_and_noncanonical_envelopes() {
    let (m, key) = fixture();
    let fp = m.approval_public_key_sha256.clone();
    let raw = envelope(m, &key);
    let value: Value = serde_json::from_slice(&raw).unwrap();
    let mut unknown = value.clone();
    unknown["other"] = json!(true);
    let changed = serde_json::to_vec(&unknown).unwrap();
    assert!(verify_bootstrap(&changed, &digest(&changed), &fp, 1000).is_err());
    let mut changed = raw.clone();
    changed.push(b'\n');
    assert!(verify_bootstrap(&changed, &digest(&changed), &fp, 1000).is_err());
    let changed = String::from_utf8(raw)
        .unwrap()
        .replacen(
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"schemaVersion\":1",
            1,
        )
        .into_bytes();
    assert!(verify_bootstrap(&changed, &digest(&changed), &fp, 1000).is_err());
}
#[test]
fn first_bootstrap_review_is_exact_and_never_a_release_review() {
    let (m, _) = fixture();
    let valid = json!({"schemaVersion":1,"kind":"INDEPENDENT_BOOTSTRAP_REVIEW","scope":PURPOSE,
        "sourceCommit":m.source_commit,"installerSha256":m.installer.sha256,"brokerArtifactSha256":m.broker_artifact.sha256,
        "reviewerIdentity":m.reviewer_identity,"independent":true,"fullBootstrapSourceReviewed":true,
        "verdict":"PASS","findingsCount":0,"neededReadsCount":0,"evidenceSha256":["9".repeat(64)]});
    validate_independent_review(&serde_json::to_vec(&valid).unwrap(), &m).unwrap();
    for (key, value) in [
        ("scope", json!("FULL_RELEASE")),
        ("installerSha256", json!("a".repeat(64))),
        ("independent", json!(false)),
        ("fullBootstrapSourceReviewed", json!(false)),
        ("findingsCount", json!(1)),
        ("neededReadsCount", json!(1)),
        ("verdict", json!("INCONCLUSIVE")),
        ("evidenceSha256", json!([])),
        ("extra", json!(true)),
    ] {
        let mut changed = valid.clone();
        changed[key] = value;
        assert!(
            validate_independent_review(&serde_json::to_vec(&changed).unwrap(), &m).is_err(),
            "{key}"
        );
    }
}
