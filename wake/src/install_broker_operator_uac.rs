// Private query-only UAC metadata witnesses. No linked token is assigned,
// impersonated, duplicated, elevated or converted into operator evidence.
use super::*;
use std::mem::{offset_of, size_of};

const INVALID: &str = "WAKE_LOCAL_UAC_PROFILE_INVALID";
const UNAVAILABLE: &str = "WAKE_LOCAL_UAC_QUERY_UNAVAILABLE";
const MISMATCH: &str = "WAKE_LOCAL_UAC_PROFILE_MISMATCH";
const CHANGED: &str = "WAKE_LOCAL_UAC_TOKEN_CHANGED";
const TOKEN_PRIVILEGES: u32 = 3;
const TOKEN_STATISTICS: u32 = 10;
const TOKEN_RESTRICTED_SIDS: u32 = 11;
const TOKEN_ELEVATION_TYPE: u32 = 18;
const TOKEN_LINKED_TOKEN: u32 = 19;
const TOKEN_MANDATORY_POLICY: u32 = 27;

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Luid {
    low: u32,
    high: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Privilege {
    luid: Luid,
    attributes: u32,
}
#[repr(C)]
struct PrivilegesHeader {
    count: u32,
    privileges: [Privilege; 1],
}
#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct Statistics {
    token_id: Luid,
    authentication_id: Luid,
    expiration_time: i64,
    token_type: u32,
    impersonation_level: u32,
    dynamic_charged: u32,
    dynamic_available: u32,
    group_count: u32,
    privilege_count: u32,
    modified_id: Luid,
}
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Group {
    sid: Vec<u8>,
    attributes: u32,
}
#[derive(Clone, Eq, PartialEq)]
struct Profile {
    user_sid: Vec<u8>,
    user_attributes: u32,
    authentication_id: Luid,
    session_id: u32,
    integrity_rid: u32,
    interactive_enabled: bool,
    app_container: bool,
    elevated: bool,
    elevation_type: u32,
    has_restrictions: bool,
    groups: Vec<Group>,
    privileges: Vec<Privilege>,
    restricting_sids: Vec<Group>,
    mandatory_policy: u32,
}
#[derive(Clone, Eq, PartialEq)]
struct Snapshot {
    facts: TokenFacts,
    profile: Profile,
    statistics: Statistics,
}

// Private constructor: only retained native observations produce this proof.
// It has no serialization, identity diagnostics or elevated-operator capability.
pub(super) struct CanonicalUacProof {
    _private: (),
}

#[cfg(test)]
thread_local! { static QUERY_ROLE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("UNSCOPED") }; }
#[cfg(test)]
struct DiagnosticRole(&'static str);
#[cfg(test)]
impl DiagnosticRole {
    fn enter(role: &'static str) -> Self {
        Self(QUERY_ROLE.with(|slot| {
            let old = slot.get();
            slot.set(role);
            old
        }))
    }
}
#[cfg(test)]
impl Drop for DiagnosticRole {
    fn drop(&mut self) {
        QUERY_ROLE.with(|slot| slot.set(self.0));
    }
}
#[cfg(test)]
pub(super) fn diagnostic_query_failure(
    class: u32,
    category: &'static str,
    native_error: u32,
    needed: u32,
    returned: u32,
) {
    QUERY_ROLE.with(|slot| eprintln!("CANONICAL_UAC_QUERY_FAILED role={} informationClass={} category={} nativeError={} neededBytes={} returnedBytes={}",slot.get(),class,category,native_error,needed,returned));
}
fn query(token: &OwnedHandle, class: u32) -> Result<Vec<u8>> {
    query_token_blob(token.as_raw_handle(), class).map_err(|_| UNAVAILABLE.into())
}
fn scalar(token: &OwnedHandle, class: u32) -> Result<u32> {
    let bytes = query(token, class)?;
    let value: [u8; 4] = bytes.as_slice().try_into().map_err(|_| INVALID)?;
    Ok(u32::from_le_bytes(value))
}
fn statistics(token: &OwnedHandle) -> Result<Statistics> {
    let bytes = query(token, TOKEN_STATISTICS)?;
    if bytes.len() != size_of::<Statistics>() {
        return Err(INVALID.into());
    }
    Ok(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<Statistics>()) })
}
fn unchanged(token: &OwnedHandle, original: &Statistics) -> Result<()> {
    let fresh = statistics(token)?;
    if fresh.token_id != original.token_id || fresh.modified_id != original.modified_id {
        return Err(CHANGED.into());
    }
    Ok(())
}
fn groups(bytes: &[u8]) -> Result<Vec<Group>> {
    let count = u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ) as usize;
    if count > MAX_TOKEN_GROUPS || bytes.len() > MAX_TOKEN_INFORMATION {
        return Err(INVALID.into());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let start = offset_of!(TokenGroupsHeader, groups);
    let stride = size_of::<SidAndAttributes>();
    let end = count
        .checked_mul(stride)
        .and_then(|n| start.checked_add(n))
        .filter(|end| *end <= bytes.len())
        .ok_or(INVALID)?;
    let mut output = Vec::with_capacity(count);
    for index in 0..count {
        let row = unsafe {
            ptr::read_unaligned(
                bytes
                    .as_ptr()
                    .add(start + index * stride)
                    .cast::<SidAndAttributes>(),
            )
        };
        if !row.sid.addr().is_multiple_of(std::mem::align_of::<u32>())
            || row.attributes & SE_GROUP_ENABLED != 0
                && row.attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0
        {
            return Err(INVALID.into());
        }
        output.push(Group {
            sid: embedded_sid_at(bytes, row.sid, end)
                .map_err(|_| INVALID)?
                .to_vec(),
            attributes: row.attributes,
        });
    }
    output.sort();
    if output.windows(2).any(|rows| rows[0].sid == rows[1].sid) {
        return Err(INVALID.into());
    }
    Ok(output)
}
fn privileges(bytes: &[u8]) -> Result<Vec<Privilege>> {
    let count = u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or(INVALID)?
            .try_into()
            .map_err(|_| INVALID)?,
    ) as usize;
    if count > 256 || bytes.len() > MAX_TOKEN_INFORMATION {
        return Err(INVALID.into());
    }
    let start = offset_of!(PrivilegesHeader, privileges);
    let stride = size_of::<Privilege>();
    let end = count
        .checked_mul(stride)
        .and_then(|n| start.checked_add(n))
        .ok_or(INVALID)?;
    if end != bytes.len() {
        return Err(INVALID.into());
    }
    let mut output = Vec::with_capacity(count);
    for index in 0..count {
        output.push(unsafe {
            ptr::read_unaligned(
                bytes
                    .as_ptr()
                    .add(start + index * stride)
                    .cast::<Privilege>(),
            )
        });
    }
    output.sort();
    if output.windows(2).any(|rows| rows[0].luid == rows[1].luid) {
        return Err(INVALID.into());
    }
    Ok(output)
}
fn user(token: &OwnedHandle) -> Result<(Vec<u8>, u32)> {
    let bytes = query(token, TOKEN_USER)?;
    if bytes.len() < size_of::<SidAndAttributes>() {
        return Err(INVALID.into());
    }
    let row = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<SidAndAttributes>()) };
    if !row.sid.addr().is_multiple_of(std::mem::align_of::<u32>()) {
        return Err(INVALID.into());
    }
    let sid = embedded_sid_at(&bytes, row.sid, size_of::<SidAndAttributes>())
        .map_err(|_| INVALID)?
        .to_vec();
    Ok((sid, row.attributes))
}
fn snapshot(token: &OwnedHandle) -> Result<Snapshot> {
    let before = statistics(token)?;
    if !matches!(before.token_type, 1 | 2) {
        return Err(INVALID.into());
    }
    let facts =
        token_facts(token.as_raw_handle(), before.token_type == 1).map_err(|_| UNAVAILABLE)?;
    let (user_sid, user_attributes) = user(token)?;
    if user_sid != facts.user_sid {
        return Err(CHANGED.into());
    }
    let normalized_groups = groups(&query(token, TOKEN_GROUPS)?)?;
    let restricting_sids = groups(&query(token, TOKEN_RESTRICTED_SIDS)?)?;
    let privileges = privileges(&query(token, TOKEN_PRIVILEGES)?)?;
    let profile = Profile {
        user_sid,
        user_attributes,
        authentication_id: before.authentication_id,
        session_id: facts.session_id,
        integrity_rid: facts.integrity_rid,
        interactive_enabled: facts.interactive_enabled,
        app_container: facts.app_container,
        elevated: facts.elevated,
        elevation_type: scalar(token, TOKEN_ELEVATION_TYPE)?,
        has_restrictions: facts.restricted,
        groups: normalized_groups,
        privileges,
        restricting_sids,
        mandatory_policy: scalar(token, TOKEN_MANDATORY_POLICY)?,
    };
    unchanged(token, &before)?;
    if before.group_count as usize != profile.groups.len()
        || before.privilege_count as usize != profile.privileges.len()
        || facts.impersonation_level
            != (before.token_type == 2).then_some(before.impersonation_level)
    {
        return Err(CHANGED.into());
    }
    Ok(Snapshot {
        facts,
        profile,
        statistics: before,
    })
}
fn linked(token: &OwnedHandle) -> Result<OwnedHandle> {
    let bytes = query(token, TOKEN_LINKED_TOKEN)?;
    if bytes.len() != size_of::<*mut c_void>() {
        return Err(INVALID.into());
    }
    let raw = unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<*mut c_void>()) };
    if raw.is_null() || raw == -1isize as *mut c_void {
        return Err(INVALID.into());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn witness_role(value: &Snapshot) -> bool {
    matches!(
        (value.statistics.token_type, value.facts.impersonation_level),
        (1, None) | (2, Some(1..=3))
    )
}
#[cfg(test)]
fn diagnostic_privileges(actor: &[Privilege], canonical: &[Privilege]) {
    let same_luid_set = actor.len() == canonical.len()
        && actor.iter().zip(canonical).all(|(a, b)| a.luid == b.luid);
    let actor_subset = actor
        .iter()
        .all(|a| canonical.iter().any(|b| a.luid == b.luid));
    let attributes_differ = actor.iter().any(|a| {
        canonical
            .iter()
            .any(|b| a.luid == b.luid && a.attributes != b.attributes)
    });
    let only_used_for_access = same_luid_set
        && attributes_differ
        && actor
            .iter()
            .zip(canonical)
            .all(|(a, b)| (a.attributes ^ b.attributes) & !0x8000_0000 == 0);
    eprintln!(
        "CANONICAL_UAC_PRIVILEGE_COMPARISON actorCount={} canonicalCount={} sameLuidSet={} actorLuidSubset={} commonAttributesDiffer={} attrsUsedForAccessOnly={}",
        actor.len(),
        canonical.len(),
        same_luid_set,
        actor_subset,
        attributes_differ,
        only_used_for_access
    );
}
fn validate(actor: &Snapshot, full: &Snapshot, limited: &Snapshot) -> Result<()> {
    let a = &actor.profile;
    let f = &full.profile;
    let l = &limited.profile;
    if !a.has_restrictions
        || a.user_attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0
        || a.elevated
        || a.elevation_type != 3
        || !(SECURITY_MANDATORY_MEDIUM_RID..SECURITY_MANDATORY_HIGH_RID).contains(&a.integrity_rid)
        || !a.restricting_sids.is_empty()
        || !witness_role(full)
        || !witness_role(limited)
        || f.elevation_type != 2
        || !f.elevated
        || f.integrity_rid < SECURITY_MANDATORY_HIGH_RID
        || f.app_container
        || f.user_sid != a.user_sid
        || f.session_id != a.session_id
        || l.elevation_type != 3
        || l.elevated
        || l.app_container
        || !l.interactive_enabled
        || l.session_id == 0
        || !l.restricting_sids.is_empty()
        || !(SECURITY_MANDATORY_MEDIUM_RID..SECURITY_MANDATORY_HIGH_RID).contains(&l.integrity_rid)
        || a != l
    {
        #[cfg(test)]
        diagnostic_privileges(&a.privileges, &l.privileges);
        #[cfg(test)]
        eprintln!(
            "CANONICAL_UAC_PROFILE_COMPARISON user={} userAttrs={} auth={} session={} integrity={} interactive={} appContainer={} elevated={} elevationType={} filterHistory={} groups={} privileges={} restrictingSids={} mandatoryPolicy={} fullRole={} limitedRole={}",
            a.user_sid == l.user_sid,
            a.user_attributes == l.user_attributes,
            a.authentication_id == l.authentication_id,
            a.session_id == l.session_id,
            a.integrity_rid == l.integrity_rid,
            a.interactive_enabled == l.interactive_enabled,
            a.app_container == l.app_container,
            a.elevated == l.elevated,
            a.elevation_type == l.elevation_type,
            a.has_restrictions == l.has_restrictions,
            a.groups == l.groups,
            a.privileges == l.privileges,
            a.restricting_sids == l.restricting_sids,
            a.mandatory_policy == l.mandatory_policy,
            witness_role(full),
            witness_role(limited)
        );
        return Err(MISMATCH.into());
    }
    // No Full authentication-ID comparison and no cross-token TokenId or
    // ModifiedId comparison: those IDs only fence each retained object's reads.
    Ok(())
}

pub(super) fn verify(
    actor_token: &OwnedHandle,
    actor_facts: &TokenFacts,
) -> Result<CanonicalUacProof> {
    let actor = {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("ACTOR_PROFILE");
        snapshot(actor_token)?
    };
    if actor.facts != *actor_facts {
        return Err(CHANGED.into());
    }
    if !actor.profile.restricting_sids.is_empty()
        || actor.profile.user_attributes & SE_GROUP_USE_FOR_DENY_ONLY != 0
        || !actor.profile.has_restrictions
        || actor.profile.elevated
        || actor.profile.elevation_type != 3
        || !(SECURITY_MANDATORY_MEDIUM_RID..SECURITY_MANDATORY_HIGH_RID)
            .contains(&actor.profile.integrity_rid)
    {
        return Err(MISMATCH.into());
    }
    let full_token = {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("ACTOR_TO_FULL_LINK");
        linked(actor_token)?
    };
    let full = {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("FULL_REFERENCE_PROFILE");
        snapshot(&full_token)?
    };
    let limited_token = {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("FULL_TO_LIMITED_LINK");
        linked(&full_token)?
    };
    let limited = {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("LIMITED_REFERENCE_PROFILE");
        snapshot(&limited_token)?
    };
    validate(&actor, &full, &limited)?;
    {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("ACTOR_FINAL_STABILITY");
        unchanged(actor_token, &actor.statistics)?;
    }
    {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("FULL_FINAL_STABILITY");
        unchanged(&full_token, &full.statistics)?;
    }
    {
        #[cfg(test)]
        let _role = DiagnosticRole::enter("LIMITED_FINAL_STABILITY");
        unchanged(&limited_token, &limited.statistics)?;
    }
    Ok(CanonicalUacProof { _private: () })
}

#[cfg(test)]
mod tests {
    use super::*;
    const USER: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 21, 0, 0, 0];
    fn fixture() -> (Snapshot, Snapshot, Snapshot) {
        let facts = TokenFacts {
            user_sid: USER.to_vec(),
            integrity_rid: 0x2000,
            session_id: 1,
            interactive_enabled: true,
            elevated: false,
            impersonation_level: None,
            app_container: false,
            restricted: true,
        };
        let stats = Statistics {
            token_id: Luid { low: 1, high: 0 },
            authentication_id: Luid { low: 2, high: 0 },
            expiration_time: 0,
            token_type: 1,
            impersonation_level: 0,
            dynamic_charged: 0,
            dynamic_available: 0,
            group_count: 1,
            privilege_count: 1,
            modified_id: Luid { low: 3, high: 0 },
        };
        let profile = Profile {
            user_sid: USER.to_vec(),
            user_attributes: 0,
            authentication_id: stats.authentication_id,
            session_id: 1,
            integrity_rid: 0x2000,
            interactive_enabled: true,
            app_container: false,
            elevated: false,
            elevation_type: 3,
            has_restrictions: true,
            groups: vec![Group {
                sid: INTERACTIVE_SID.to_vec(),
                attributes: 4,
            }],
            privileges: vec![Privilege {
                luid: Luid { low: 23, high: 0 },
                attributes: 2,
            }],
            restricting_sids: vec![],
            mandatory_policy: 1,
        };
        let actor = Snapshot {
            facts,
            profile,
            statistics: stats,
        };
        let mut limited = actor.clone();
        limited.statistics.token_id.low = 4;
        limited.statistics.modified_id.low = 5;
        limited.statistics.token_type = 2;
        limited.statistics.impersonation_level = 1;
        limited.facts.impersonation_level = Some(1);
        let mut full = limited.clone();
        full.statistics.token_id.low = 6;
        full.statistics.authentication_id.low = 99;
        full.profile.authentication_id = full.statistics.authentication_id;
        full.profile.elevated = true;
        full.profile.elevation_type = 2;
        full.profile.integrity_rid = 0x3000;
        full.facts.elevated = true;
        full.facts.integrity_rid = 0x3000;
        (actor, full, limited)
    }
    #[test]
    fn canonical_uac_metadata_identification_witnesses_do_not_require_primary_or_cross_token_ids() {
        let (actor, full, limited) = fixture();
        assert!(validate(&actor, &full, &limited).is_ok());
        assert!(authorize_token_facts(&actor.facts, &USER, true).is_err());
        assert!(authorize_local_user_facts(&limited.facts, &USER, false).is_err());
    }
    #[test]
    fn canonical_uac_exact_profile_rejects_further_sid_privilege_auth_and_policy_filters() {
        for variant in 0..24 {
            let (mut actor, mut full, mut limited) = fixture();
            match variant {
                0 => actor.profile.groups[0].attributes = SE_GROUP_USE_FOR_DENY_ONLY,
                1 => actor.profile.privileges.clear(),
                2 => actor.profile.privileges[0].attributes = 0,
                3 => actor.profile.authentication_id.low += 1,
                4 => actor.profile.session_id += 1,
                5 => actor.profile.user_sid[8] ^= 1,
                6 => actor.profile.mandatory_policy = 0,
                7 => actor.profile.restricting_sids = actor.profile.groups.clone(),
                8 => actor.profile.elevated = true,
                9 => actor.profile.elevation_type = 1,
                10 => actor.profile.integrity_rid = 0x1000,
                11 => actor.profile.app_container = true,
                12 => actor.profile.has_restrictions = false,
                13 => full.profile.user_sid[8] ^= 1,
                14 => full.profile.session_id += 1,
                15 => full.profile.elevated = false,
                16 => full.profile.integrity_rid = 0x2000,
                17 => limited.profile.integrity_rid = 0x3000,
                18 => limited.profile.elevated = true,
                19 => limited.statistics.token_type = 3,
                20 => limited.facts.impersonation_level = Some(0),
                21 => actor.profile.interactive_enabled = false,
                22 => actor.profile.user_attributes = SE_GROUP_USE_FOR_DENY_ONLY,
                _ => {
                    actor.profile.user_attributes = SE_GROUP_USE_FOR_DENY_ONLY;
                    limited.profile.user_attributes = SE_GROUP_USE_FOR_DENY_ONLY;
                }
            }
            assert!(
                validate(&actor, &full, &limited).is_err(),
                "profile variant {variant}"
            );
        }
    }
    #[test]
    fn canonical_uac_privilege_parser_refuses_truncation_duplicate_and_oversized_counts() {
        let mut valid = Vec::new();
        valid.extend(1u32.to_le_bytes());
        valid.extend(23u32.to_le_bytes());
        valid.extend(0u32.to_le_bytes());
        valid.extend(2u32.to_le_bytes());
        assert!(privileges(&valid).is_ok());
        for variant in 0..3 {
            let mut bytes = valid.clone();
            match variant {
                0 => {
                    bytes.pop();
                }
                1 => bytes[..4].copy_from_slice(&257u32.to_le_bytes()),
                _ => {
                    bytes[..4].copy_from_slice(&2u32.to_le_bytes());
                    bytes.extend_from_slice(&valid[4..]);
                }
            }
            assert!(privileges(&bytes).is_err());
        }
    }
    #[test]
    fn canonical_uac_group_normalizer_refuses_duplicate_contradictory_and_outside_rows() {
        use super::super::tests::groups_blob;
        let valid = groups_blob(&[(&USER, 4), (&INTERACTIVE_SID, 4)], 0);
        assert!(groups(&valid).is_ok());
        for variant in 0..5 {
            let mut bytes = if variant == 1 {
                groups_blob(&[(&USER, 4), (&USER, 0)], 0)
            } else {
                groups_blob(&[(&USER, if variant == 3 { 4 | 16 } else { 4 })], 0)
            };
            match variant {
                0 => bytes[..4].copy_from_slice(&257u32.to_le_bytes()),
                2 => unsafe {
                    ptr::write_unaligned(
                        bytes
                            .as_mut_ptr()
                            .add(offset_of!(TokenGroupsHeader, groups))
                            .cast::<SidAndAttributes>(),
                        SidAndAttributes {
                            sid: ptr::dangling_mut(),
                            attributes: 4,
                        },
                    )
                },
                4 => {
                    bytes.pop();
                }
                _ => {}
            }
            assert!(groups(&bytes).is_err(), "group variant {variant}");
        }
    }
    fn same_security(a: &Snapshot, b: &Snapshot) -> bool {
        a.facts == b.facts
            && a.profile == b.profile
            && a.statistics.token_id == b.statistics.token_id
            && a.statistics.modified_id == b.statistics.modified_id
    }
    fn current(access: u32) -> OwnedHandle {
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut raw) },
            0
        );
        assert!(!raw.is_null());
        unsafe { OwnedHandle::from_raw_handle(raw) }
    }
    #[test]
    fn canonical_uac_current_primary_uses_actual_complete_os_profile_without_elevation() {
        let token = current(TOKEN_QUERY);
        let before = snapshot(&token).unwrap();
        let result = inspect_current_process_local_user(&before.facts.user_sid);
        let basic = require_user_identity_facts(
            &before.facts,
            &before.facts.user_sid,
            true,
            SECURITY_MANDATORY_MEDIUM_RID,
            false,
        )
        .is_ok();
        if basic {
            assert!(
                result.is_ok(),
                "actual current local actor must qualify or report a real unsupported context"
            );
            if before.facts.restricted {
                assert!(verify(&token, &before.facts).is_ok());
                eprintln!("CANONICAL_UAC_CURRENT_PROFILE_MATCHED=true");
            }
        } else {
            assert!(result.is_err());
            eprintln!("CANONICAL_UAC_CURRENT_IDENTITY_ELIGIBLE=false");
        }
        assert!(
            same_security(&snapshot(&token).unwrap(), &before),
            "original current token must remain unchanged"
        );
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn CreateRestrictedToken(
            existing: *mut c_void,
            flags: u32,
            disable_count: u32,
            disable: *const SidAndAttributes,
            delete_count: u32,
            delete: *const Privilege,
            restrict_count: u32,
            restrict: *const SidAndAttributes,
            new_token: *mut *mut c_void,
        ) -> i32;
    }
    #[test]
    fn canonical_uac_disposable_further_filtered_tokens_never_gain_local_admission() {
        let source = current(TOKEN_QUERY | 2);
        let before = snapshot(&source).unwrap();
        if !before.facts.restricted
            || before.profile.elevation_type != 3
            || !before.profile.interactive_enabled
            || before.profile.session_id == 0
        {
            eprintln!("CANONICAL_UAC_FURTHER_FILTER_CONTEXT_AVAILABLE=false");
            return;
        }
        assert!(
            verify(&source, &before.facts).is_ok(),
            "base canonical actor is required for further-filter negative controls"
        );
        let group = before
            .profile
            .groups
            .iter()
            .find(|g| {
                g.attributes & SE_GROUP_ENABLED != 0
                    && g.attributes & 0x20 == 0
                    && g.sid != INTERACTIVE_SID
                    && g.sid != before.profile.user_sid
            })
            .expect("an enabled non-integrity noninteractive group");
        let aligned = |sid: &[u8]| {
            sid.chunks_exact(4)
                .map(|word| u32::from_ne_bytes(word.try_into().expect("SID DWORD")))
                .collect::<Vec<_>>()
        };
        let disable_sid = aligned(&group.sid);
        let disabled_user_sid = aligned(&before.profile.user_sid);
        let restricting_sid = aligned(&INTERACTIVE_SID);
        let disable = SidAndAttributes {
            sid: disable_sid.as_ptr().cast_mut().cast(),
            attributes: 0,
        };
        let disable_user = SidAndAttributes {
            sid: disabled_user_sid.as_ptr().cast_mut().cast(),
            attributes: 0,
        };
        let privilege = *before
            .profile
            .privileges
            .first()
            .expect("a privilege to remove");
        let restricted = SidAndAttributes {
            sid: restricting_sid.as_ptr().cast_mut().cast(),
            attributes: 0,
        };
        for variant in 0..4 {
            let mut raw = ptr::null_mut();
            assert_ne!(
                unsafe {
                    CreateRestrictedToken(
                        source.as_raw_handle(),
                        0,
                        u32::from(variant == 0 || variant == 3),
                        if variant == 0 {
                            &disable
                        } else if variant == 3 {
                            &disable_user
                        } else {
                            ptr::null()
                        },
                        u32::from(variant == 1),
                        if variant == 1 {
                            &privilege
                        } else {
                            ptr::null()
                        },
                        u32::from(variant == 2),
                        if variant == 2 {
                            &restricted
                        } else {
                            ptr::null()
                        },
                        &mut raw,
                    )
                },
                0
            );
            assert!(!raw.is_null());
            let isolated = unsafe { OwnedHandle::from_raw_handle(raw) };
            let facts = token_facts(isolated.as_raw_handle(), true).unwrap();
            assert!(
                authorize_local_user_token(&isolated, &facts, &before.facts.user_sid, true)
                    .is_err(),
                "further filter {variant} must not qualify"
            );
            assert!(
                same_security(&snapshot(&source).unwrap(), &before),
                "original source token unchanged"
            );
        }
        eprintln!("CANONICAL_UAC_FURTHER_FILTER_NEGATIVES_CHECKED=4");
    }
}
