use sha2::{Digest, Sha256};

#[test]
fn t0215_bootstrap_policy_digest_vector_is_stable() {
    let policy = r"CATDESK_REVIEWED_MAIN_IMAGE_BOOTSTRAP_V1_OPENED_REVIEWED_ARTIFACT_ONLY|C:\Program Files\CatDesk\CatDesk.exe";
    assert_eq!(
        format!("{:x}", Sha256::digest(policy.as_bytes())),
        "a89cccc440eabe56997dab3fa064f62c1d5b11a9704c50286e506d9dfdd51efb"
    );
}

#[test]
fn t0217_rotation_policy_digest_vector_is_stable() {
    let policy = r"CATDESK_REVIEWED_MAIN_IMAGE_ROTATION_V1_MONOTONIC_SIGNED_ATOMIC_REPLACE|C:\Program Files\CatDesk\CatDesk.exe|C:\Program Files\CatDesk\CatDesk.rotation-next.exe|C:\Program Files\CatDesk.reviewed-main-image-rotation-pending.v1|C:\Program Files\CatDesk.reviewed-main-image-rotation-installed.v1";
    assert_eq!(
        format!("{:x}", Sha256::digest(policy.as_bytes())),
        "c477d5bc486002e2034324b3a40b07d8e87c149cf5f2dab9b12aafee92816705"
    );
}
