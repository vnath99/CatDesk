//! Retained V2 first-pointer readback through the existing no-follow reader.
//! Exact plan bytes and the published package receipt are both required; a
//! matching JSON document alone is not install, owner or launch authority.

use super::*;
use crate::{
    install_broker_ledger::{
        OfflineLedgerLeaseV2, OfflinePointerBindingV2, OfflinePointerPlanV2, StageBindingV2,
    },
    install_broker_offline_pins::full_identity_of_handle,
    install_broker_offline_v2::OfflineObjectIdentityV2,
    install_broker_stage_writer::BrokerPublishedReceiptV2,
};

const MAX_OFFLINE_POINTER_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedInstallReadOnlySnapshotV2 {
    pub schema_version: u32,
    pub install_generation: u64,
    pub protected_root: OfflineObjectIdentityV2,
    pub pointer_binding: OfflinePointerBindingV2,
    pub package_id: String,
    pub version: String,
    pub manifest_sha256: String,
    pub stage_binding: StageBindingV2,
    pub disposition: &'static str,
}

pub struct ProtectedInstallPinV2<'p> {
    pointer: File,
    plan: OfflinePointerPlanV2,
    security: ExactSecurity,
    snapshot: ProtectedInstallReadOnlySnapshotV2,
    published: BrokerPublishedReceiptV2<'p>,
}

fn pointer_identity(file: &File) -> Result<OfflineObjectIdentityV2> {
    Ok(OfflineObjectIdentityV2 {
        legacy: identity(&file_info(file.as_raw_handle(), false)?),
        full: full_identity_of_handle(file.as_raw_handle())?,
    })
}

fn require_pointer_policy(
    lease: &OfflineLedgerLeaseV2<'_, '_>,
    security: &ExactSecurity,
) -> Result<()> {
    if let Some(policy) = lease.policy() {
        policy.revalidate()?;
        let (sid, _) = policy.checked_endpoint_sids()?;
        if security.owner != "S-1-5-18"
            || security.sddl != canonical_protected_security(sid, false)?
        {
            return Err("WAKE_OFFLINE_POINTER_ACL_MISMATCH".into());
        }
    } else {
        #[cfg(not(test))]
        return Err("WAKE_OFFLINE_POINTER_POLICY_REQUIRED".into());
    }
    Ok(())
}

impl<'p> ProtectedInstallPinV2<'p> {
    pub fn snapshot(&self) -> &ProtectedInstallReadOnlySnapshotV2 {
        &self.snapshot
    }
    pub fn pointer_binding(&self) -> &OfflinePointerBindingV2 {
        &self.snapshot.pointer_binding
    }
    pub(crate) fn published_mut(&mut self) -> &mut BrokerPublishedReceiptV2<'p> {
        &mut self.published
    }

    pub fn revalidate(&mut self, lease: &OfflineLedgerLeaseV2<'_, '_>) -> Result<()> {
        lease.revalidate_inputs()?;
        if lease.checked_initial_pointer_plan()? != self.plan
            || self.snapshot.protected_root != *lease.protected_root_identity()
            || self.snapshot.package_id != lease.approved_manifest().package_id
            || self.snapshot.version != lease.approved_manifest().version
            || self.snapshot.manifest_sha256 != lease.approved_manifest_sha256()
            || &self.snapshot.stage_binding != lease.stage_binding_for_recovery()?
        {
            return Err("WAKE_OFFLINE_POINTER_BINDING_MISMATCH".into());
        }
        require_pointer_policy(lease, &self.security)?;
        let binding = self.published.binding_for_ledger(
            lease.protected_root(),
            lease.protected_root_identity(),
            lease.intent_record_sha256(),
            lease.approved_manifest(),
            lease.approved_manifest_sha256(),
        )?;
        if binding != self.snapshot.stage_binding {
            return Err("WAKE_OFFLINE_POINTER_BINDING_MISMATCH".into());
        }
        if pointer_identity(&self.pointer)? != self.snapshot.pointer_binding.pointer_file {
            return Err("WAKE_OFFLINE_POINTER_OBJECT_CHANGED".into());
        }
        let (bytes, _) =
            read_bounded(&mut self.pointer, MAX_OFFLINE_POINTER_BYTES, &self.security)?;
        if bytes != self.plan.canonical_bytes()
            || format!("{:x}", Sha256::digest(&bytes))
                != self.snapshot.pointer_binding.pointer_sha256
        {
            return Err("WAKE_OFFLINE_POINTER_BYTES_MISMATCH".into());
        }
        let mut reopened = File::from(open_relative(
            lease.protected_root(),
            "current.json",
            false,
        )?);
        if pointer_identity(&reopened)? != self.snapshot.pointer_binding.pointer_file {
            return Err("WAKE_OFFLINE_POINTER_OBJECT_CHANGED".into());
        }
        let (again, _) = read_bounded(&mut reopened, MAX_OFFLINE_POINTER_BYTES, &self.security)?;
        if again != bytes
            || pointer_identity(&self.pointer)? != self.snapshot.pointer_binding.pointer_file
        {
            return Err("WAKE_OFFLINE_POINTER_BYTES_MISMATCH".into());
        }
        require_pointer_policy(lease, &self.security)?;
        lease.revalidate_inputs()
    }
}

pub(crate) fn pin_created_offline_pointer<'p>(
    lease: &OfflineLedgerLeaseV2<'_, 'p>,
    published: BrokerPublishedReceiptV2<'p>,
    plan: &OfflinePointerPlanV2,
    expected: &OfflineObjectIdentityV2,
    expected_security: (String, String),
) -> Result<ProtectedInstallPinV2<'p>> {
    if lease.checked_initial_pointer_plan()? != *plan {
        return Err("WAKE_OFFLINE_POINTER_BINDING_MISMATCH".into());
    }
    let pointer = File::from(open_relative(
        lease.protected_root(),
        "current.json",
        false,
    )?);
    if pointer_identity(&pointer)? != *expected {
        return Err("WAKE_OFFLINE_POINTER_OBJECT_CHANGED".into());
    }
    let security = ExactSecurity {
        owner: expected_security.0,
        sddl: expected_security.1,
    };
    assert_security(pointer.as_raw_handle(), &security)?;
    require_pointer_policy(lease, &security)?;
    let snapshot = ProtectedInstallReadOnlySnapshotV2 {
        schema_version: 2,
        install_generation: 1,
        protected_root: lease.protected_root_identity().clone(),
        pointer_binding: OfflinePointerBindingV2 {
            pointer_file: expected.clone(),
            pointer_sha256: plan.sha256().into(),
        },
        package_id: lease.approved_manifest().package_id.clone(),
        version: lease.approved_manifest().version.clone(),
        manifest_sha256: lease.approved_manifest_sha256().into(),
        stage_binding: lease.stage_binding_for_recovery()?.clone(),
        disposition: "EXACT_OFFLINE_POINTER_READ_ONLY_NO_LAUNCH_AUTHORITY",
    };
    let mut pin = ProtectedInstallPinV2 {
        pointer,
        plan: plan.clone(),
        security,
        snapshot,
        published,
    };
    pin.revalidate(lease)?;
    Ok(pin)
}

/// Recover only the exact pointer ID/hash already bound in complete journal
/// history. Production security is re-derived from fixed protected policy,
/// not accepted from the current file or a serialized caller observation.
pub(crate) fn reopen_recovered_offline_pointer<'p>(
    lease: &OfflineLedgerLeaseV2<'_, 'p>,
    published: BrokerPublishedReceiptV2<'p>,
    expected: &OfflinePointerBindingV2,
) -> Result<ProtectedInstallPinV2<'p>> {
    let plan = lease.checked_initial_pointer_plan()?;
    if plan.sha256() != expected.pointer_sha256 {
        return Err("WAKE_OFFLINE_POINTER_BINDING_MISMATCH".into());
    }
    // Keep this no-write/delete pin live across security selection and the
    // second complete pointer/package readback below.
    let first = open_relative(lease.protected_root(), "current.json", false)?;
    let security = if let Some(policy) = lease.policy() {
        policy.revalidate()?;
        (
            "S-1-5-18".to_owned(),
            canonical_protected_security(policy.checked_endpoint_sids()?.0, false)?,
        )
    } else {
        #[cfg(not(test))]
        return Err("WAKE_OFFLINE_POINTER_POLICY_REQUIRED".into());
        #[cfg(test)]
        {
            protected_security_raw(first.as_raw_handle())?
        }
    };
    let pin =
        pin_created_offline_pointer(lease, published, &plan, &expected.pointer_file, security)?;
    drop(first);
    Ok(pin)
}
