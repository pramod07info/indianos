//! Policy preflight: signature verification happens in a separate trusted component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Package {
    pub signature_verified: bool,
    pub signer_id: u32,
    pub version: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageError { Unverified, UntrustedSigner, Downgrade }

/// A true signature_verified value must come from a trusted verifier, not the package.
pub fn permit(pkg: Package, trusted_signer: u32, minimum_version: u64)
    -> Result<(), PackageError>
{
    if !pkg.signature_verified { return Err(PackageError::Unverified); }
    if trusted_signer == 0 || pkg.signer_id != trusted_signer {
        return Err(PackageError::UntrustedSigner);
    }
    if pkg.version < minimum_version { return Err(PackageError::Downgrade); }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_current_version_passes() {
        assert_eq!(permit(Package { signature_verified: true, signer_id: 7, version: 9 }, 7, 9), Ok(()));
    }
    #[test]
    fn unsigned_or_untrusted_is_rejected() {
        let mut pkg = Package { signature_verified: false, signer_id: 7, version: 9 };
        assert_eq!(permit(pkg, 7, 9), Err(PackageError::Unverified));
        pkg.signature_verified = true;
        assert_eq!(permit(pkg, 0, 9), Err(PackageError::UntrustedSigner));
        assert_eq!(permit(pkg, 8, 9), Err(PackageError::UntrustedSigner));
    }
    #[test]
    fn downgrade_and_large_version_bounds() {
        assert_eq!(permit(Package { signature_verified: true, signer_id: 7, version: 8 }, 7, 9),
            Err(PackageError::Downgrade));
        assert_eq!(permit(Package { signature_verified: true, signer_id: 7, version: u64::MAX },
            7, u64::MAX), Ok(()));
    }
}

/// Require a signature result from a trusted verifier and a signer contained
/// in an explicit local allowlist; package-provided trust keys are ignored.
pub fn permit_from_trust_store(pkg: Package, allowed_signers: &[u32],
    minimum_version: u64) -> Result<(), PackageError>
{
    if !pkg.signature_verified { return Err(PackageError::Unverified); }
    if pkg.signer_id == 0 || !allowed_signers.contains(&pkg.signer_id) {
        return Err(PackageError::UntrustedSigner);
    }
    permit(pkg, pkg.signer_id, minimum_version)
}
#[cfg(test)]
mod trust_store_tests {
    use super::*;
    fn pkg() -> Package {
        Package { signature_verified: true, signer_id: 42, version: 9 }
    }
    #[test]
    fn allows_only_local_trusted_signers() {
        assert_eq!(permit_from_trust_store(pkg(), &[7,42], 9), Ok(()));
        assert_eq!(permit_from_trust_store(pkg(), &[], 0), Err(PackageError::UntrustedSigner));
    }
    #[test]
    fn rejects_unsigned_even_when_signer_exists() {
        let mut item = pkg();
        item.signature_verified = false;
        assert_eq!(permit_from_trust_store(item, &[42], 0), Err(PackageError::Unverified));
        item.signature_verified = true;
        item.signer_id = 0;
        assert_eq!(permit_from_trust_store(item, &[0], 0), Err(PackageError::UntrustedSigner));
    }
    #[test]
    fn minimum_version_enforces_downgrade_prevention() {
        assert_eq!(permit_from_trust_store(pkg(), &[42], 10), Err(PackageError::Downgrade));
    }
}


/// Guest-owned trust root. The public key comes from immutable trusted
/// configuration, never an app-provided self-signed manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustAnchor { pub id: u32, pub ed25519_public_key: [u8; 32] }

/// An independent cryptographic verifier must implement this interface.
/// No implementation is shipped in this helper: it must fail closed when a
/// real Ed25519 implementation is unavailable on the target.
pub trait GuestSignatureVerifier {
    fn verify(&self, public_key: &[u8; 32], signed_bytes: &[u8],
              signature: &[u8; 64]) -> bool;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestPackageError { UntrustedKey, Unsigned, Downgrade, WrongArchitecture, Malformed }

/// Verified preflight for a single small staged manifest. Returns no runnable
/// ELF handle; a separate privileged loader must validate the real payload.
pub fn verify_guest_manifest<V: GuestSignatureVerifier>(
    verifier: &V,
    anchors: &[TrustAnchor], key_id: u32,
    manifest: &[u8], signature: &[u8; 64], version: u64,
    target_arch: u16, host_arch: u16, min_version: u64,
) -> Result<(), GuestPackageError> {
    if manifest.is_empty() || manifest.len() > 4096 || version == 0 {
        return Err(GuestPackageError::Malformed);
    }
    if target_arch != host_arch { return Err(GuestPackageError::WrongArchitecture); }
    if version < min_version { return Err(GuestPackageError::Downgrade); }
    // Never accept ambiguous signer IDs, zero keys or attacker-sized trust
    // lists: first-match semantics silently change the effective trust root.
    if anchors.is_empty() || anchors.len()>32 {return Err(GuestPackageError::Malformed);}
    for (i,key) in anchors.iter().enumerate(){
        if key.id==0 || key.ed25519_public_key==[0;32]{
            return Err(GuestPackageError::UntrustedKey);
        }
        if anchors[..i].iter().any(|prev| prev.id==key.id){
            return Err(GuestPackageError::Malformed);
        }
    }
    let anchor = anchors.iter().find(|key| key.id==key_id)
        .ok_or(GuestPackageError::UntrustedKey)?;
    if !verifier.verify(&anchor.ed25519_public_key, manifest, signature) {
        return Err(GuestPackageError::Unsigned);
    }
    Ok(())
}
#[cfg(test)]
mod guest_verifier_tests {
    use super::*;
    struct FakeVerifier;
    impl GuestSignatureVerifier for FakeVerifier {
        fn verify(&self, pubkey: &[u8; 32], msg: &[u8], signature: &[u8; 64]) -> bool {
            // Test-only stand-in, deliberately NOT an Ed25519 implementation.
            pubkey[0] == 42 && msg == b"valid-test-manifest" && signature[0] == 0xab
        }
    }
    fn anchors() -> [TrustAnchor; 1] {
        [TrustAnchor { id: 7, ed25519_public_key: [42; 32] }]
    }
    #[test]
    fn accepts_only_explicit_trusted_test_verification() {
        let mut sig = [0u8; 64]; sig[0] = 0xab;
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"valid-test-manifest", &sig, 2, 0x8664, 0x8664, 1), Ok(()));
    }
    #[test]
    fn unknown_signer_and_tampering_are_rejected() {
        let mut sig = [0u8; 64]; sig[0] = 0xab;
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 8,
            b"valid-test-manifest", &sig, 2, 1, 1, 1), Err(GuestPackageError::UntrustedKey));
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"tampered-manifest", &sig, 2, 1, 1, 1), Err(GuestPackageError::Unsigned));
        sig[0] = 0;
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"valid-test-manifest", &sig, 2, 1, 1, 1), Err(GuestPackageError::Unsigned));
    }
    #[test]
    fn architecture_downgrade_and_empty_input_fail_closed() {
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"valid-test-manifest", &[0; 64], 2, 1, 2, 1),
            Err(GuestPackageError::WrongArchitecture));
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"valid-test-manifest", &[0; 64], 2, 1, 1, 3),
            Err(GuestPackageError::Downgrade));
        assert_eq!(verify_guest_manifest(&FakeVerifier, &anchors(), 7,
            b"", &[0; 64], 2, 1, 1, 1), Err(GuestPackageError::Malformed));
    }
}


/// Batch 1: a separate trusted implementation must compute the actual
/// payload digest; the expected hash MUST come from the verified manifest.
pub trait GuestPayloadVerifier {
    fn digest_matches(&self, payload: &[u8], verified_manifest_digest: &[u8; 32]) -> bool;
}

/// Fail-closed manifest AND executable-payload preflight. A positive result is
/// not a launch: privileged loader, on-device Ed25519 and durable trust roots
/// remain separate requirements.
pub fn verify_launch_material<V: GuestSignatureVerifier, P: GuestPayloadVerifier>(
    verifier: &V, payload_verifier: &P, anchors: &[TrustAnchor], key_id: u32,
    manifest: &[u8], signature: &[u8;64], payload: &[u8],
    digest_from_verified_manifest: &[u8;32], version: u64,
    target_arch: u16, host_arch: u16, minimum: u64,
) -> Result<(), GuestPackageError> {
    if payload.is_empty() || payload.len() > 1024 * 1024 {
        return Err(GuestPackageError::Malformed);
    }
    verify_guest_manifest(verifier, anchors, key_id, manifest, signature,
        version, target_arch, host_arch, minimum)?;
    if !payload_verifier.digest_matches(payload, digest_from_verified_manifest) {
        return Err(GuestPackageError::Unsigned);
    }
    Ok(())
}
#[cfg(test)]
mod batch1_payload_tests {
    use super::*;
    struct TestVerifier;
    impl GuestSignatureVerifier for TestVerifier {
        fn verify(&self, key: &[u8;32], manifest: &[u8], sig: &[u8;64]) -> bool {
            key[0] == 5 && manifest == b"trusted" && sig[0] == 9
        }
    }
    struct TestDigest;
    impl GuestPayloadVerifier for TestDigest {
        fn digest_matches(&self, payload: &[u8], digest: &[u8;32]) -> bool {
            payload == b"expected-elf" && digest[0] == 7
        }
    }
    fn check(payload: &[u8], digest: [u8;32], signature: [u8;64])
        -> Result<(), GuestPackageError> {
        verify_launch_material(&TestVerifier, &TestDigest,
            &[TrustAnchor{id:1, ed25519_public_key:[5;32]}],1,
            b"trusted", &signature, payload, &digest, 2, 62, 62, 1)
    }
    #[test]
    fn validates_both_signed_manifest_and_bound_payload() {
        let mut sig=[0;64];sig[0]=9;
        let mut digest=[0;32];digest[0]=7;
        assert_eq!(check(b"expected-elf",digest,sig),Ok(()));
        assert_eq!(check(b"altered-elf",digest,sig),Err(GuestPackageError::Unsigned));
    }
    #[test]
    fn rejects_unsigned_manifest_or_empty_payload() {
        let mut digest=[0;32];digest[0]=7;
        assert_eq!(check(b"expected-elf",digest,[0;64]),Err(GuestPackageError::Unsigned));
        assert_eq!(check(b"",digest,[0;64]),Err(GuestPackageError::Malformed));
    }
}



/// Validate a proposed signer rotation for continuity and non-reuse of
/// existing key IDs. Trusted authorization of the proposed list is separate.
pub fn validate_trust_rotation(old:&[TrustAnchor],next:&[TrustAnchor],
    revoked:&[u32])->Result<(),GuestPackageError>{
    if old.is_empty()||next.is_empty(){return Err(GuestPackageError::Malformed)}
    for (i,k) in next.iter().enumerate(){
        if k.id==0||k.ed25519_public_key==[0;32]||revoked.contains(&k.id){
            return Err(GuestPackageError::UntrustedKey)
        }
        if next[..i].iter().any(|x|x.id==k.id){
            return Err(GuestPackageError::Malformed)
        }
        if old.iter().any(|x|x.id==k.id&&
           x.ed25519_public_key!=k.ed25519_public_key){
            return Err(GuestPackageError::UntrustedKey)
        }
    }
    for (i,k) in old.iter().enumerate(){
        if k.id==0||old[..i].iter().any(|x|x.id==k.id){
            return Err(GuestPackageError::Malformed)
        }
    }
    if !old.iter().any(|k|next.contains(k)&&!revoked.contains(&k.id)){
        return Err(GuestPackageError::UntrustedKey)
    }
    Ok(())
}
#[cfg(test)]
mod rotation_tests {
    use super::*;
    fn key(id:u32,b:u8)->TrustAnchor{
        TrustAnchor{id,ed25519_public_key:[b;32]}
    }
    #[test]fn retained_trusted_key_allows_addition(){
        assert_eq!(validate_trust_rotation(&[key(2,3)],
                   &[key(2,3),key(4,5)],&[]),Ok(()));
    }
    #[test]fn signer_rekeying_duplicates_and_revocation_are_denied(){
        assert_eq!(validate_trust_rotation(&[key(2,3)],&[key(2,4)],&[]),
                   Err(GuestPackageError::UntrustedKey));
        assert_eq!(validate_trust_rotation(&[key(2,3)],
                   &[key(2,3),key(2,3)],&[]),Err(GuestPackageError::Malformed));
        assert_eq!(validate_trust_rotation(&[key(2,3)],&[key(2,3)],&[2]),
                   Err(GuestPackageError::UntrustedKey));
    }
    #[test]fn empty_or_discontinuous_rotation_fails_closed(){
        assert_eq!(validate_trust_rotation(&[],&[key(2,3)],&[]),
                   Err(GuestPackageError::Malformed));
        assert_eq!(validate_trust_rotation(&[key(2,3)],&[key(4,5)],&[]),
                   Err(GuestPackageError::UntrustedKey));
        assert_eq!(validate_trust_rotation(&[key(2,3)],&[key(2,3),key(0,1)],&[]),
                   Err(GuestPackageError::UntrustedKey));
    }
}


/// Disallow rollback of authenticated trust-store epochs.
/// Authentication itself must occur in the independent trusted verifier.
pub fn validate_trust_epoch(current:u64,proposed:u64,authenticated:bool)
 ->Result<u64,GuestPackageError>{
 if !authenticated||current==0||proposed==0{return Err(GuestPackageError::Unsigned)}
 if proposed<=current{return Err(GuestPackageError::Downgrade)}
 Ok(proposed)
}
#[cfg(test)]mod trust_epoch_tests{
 use super::*;
 #[test]fn allows_verified_monotonic_update(){assert_eq!(validate_trust_epoch(2,3,true),Ok(3));}
 #[test]fn denies_unverified_proposals(){assert_eq!(validate_trust_epoch(2,4,false),Err(GuestPackageError::Unsigned));assert_eq!(validate_trust_epoch(0,2,true),Err(GuestPackageError::Unsigned));}
 #[test]fn rejects_rollback_and_epoch_reuse(){assert_eq!(validate_trust_epoch(5,5,true),Err(GuestPackageError::Downgrade));assert_eq!(validate_trust_epoch(5,4,true),Err(GuestPackageError::Downgrade));}
}


#[cfg(test)]
mod strict_anchor_tests {
    use super::*;
    struct Allow;
    impl GuestSignatureVerifier for Allow {
        fn verify(&self,_:&[u8;32],_:&[u8],_:&[u8;64])->bool {true}
    }
    fn a(id:u32,byte:u8)->TrustAnchor{
        TrustAnchor{id,ed25519_public_key:[byte;32]}
    }
    fn check(keys:&[TrustAnchor])->Result<(),GuestPackageError>{
        verify_guest_manifest(&Allow,keys,7,b"manifest",&[9;64],2,62,62,1)
    }
    #[test]fn canonical_single_anchor_remains_valid(){
        assert_eq!(check(&[a(7,42)]),Ok(()));
    }
    #[test]fn duplicate_and_oversized_roots_are_rejected(){
        assert_eq!(check(&[a(7,42),a(7,43)]),Err(GuestPackageError::Malformed));
        assert_eq!(check(&[a(7,42);33]),Err(GuestPackageError::Malformed));
    }
    #[test]fn zero_key_and_foreign_signer_fail_closed(){
        assert_eq!(check(&[a(7,0)]),Err(GuestPackageError::UntrustedKey));
        assert_eq!(check(&[a(8,42)]),Err(GuestPackageError::UntrustedKey));
    }
}


// R007: multi-party manifest endorsement; uses the separate, injected real
// signature verifier and locally provisioned trust anchors. Test doubles
// below are NOT cryptographic signing or verification implementations.
#[derive(Clone,Copy)]
pub struct ManifestVote{pub key_id:u32,pub signature:[u8;64]}
pub fn verify_manifest_quorum<V:GuestSignatureVerifier>(
    verifier:&V,anchors:&[TrustAnchor],votes:&[ManifestVote],quorum:usize,
    manifest:&[u8],version:u64,arch:u16,host:u16,min_version:u64)
    ->Result<usize,GuestPackageError>{
    if quorum==0||quorum>anchors.len()||votes.len()<quorum||
       votes.len()>anchors.len()||votes.len()>32{
        return Err(GuestPackageError::Malformed);
    }
    for (i,vote) in votes.iter().enumerate(){
        if vote.key_id==0||votes[..i].iter().any(|prior|prior.key_id==vote.key_id){
            return Err(GuestPackageError::Malformed);
        }
        verify_guest_manifest(verifier,anchors,vote.key_id,manifest,
            &vote.signature,version,arch,host,min_version)?;
    }
    Ok(votes.len())
}
#[cfg(test)]mod r007_quorum_tests{
    use super::*;
    struct TestOnlyVerifier;
    impl GuestSignatureVerifier for TestOnlyVerifier{
        fn verify(&self,key:&[u8;32],msg:&[u8],sig:&[u8;64])->bool{
            msg==b"approved" && sig[0]==key[0]
        }
    }
    fn roots()->[TrustAnchor;3]{
        [TrustAnchor{id:7,ed25519_public_key:[7;32]},
         TrustAnchor{id:8,ed25519_public_key:[8;32]},
         TrustAnchor{id:9,ed25519_public_key:[9;32]}]
    }
    fn votes()->[ManifestVote;2]{
        [ManifestVote{key_id:7,signature:[7;64]},
         ManifestVote{key_id:8,signature:[8;64]}]
    }
    fn check(v:&[ManifestVote],q:usize,version:u64)
       ->Result<usize,GuestPackageError>{
        verify_manifest_quorum(&TestOnlyVerifier,&roots(),v,q,
            b"approved",version,0x8664,0x8664,4)
    }
    #[test]fn distinct_real_verifier_callbacks_establish_quorum(){
        assert_eq!(check(&votes(),2,4),Ok(2));
        assert_eq!(check(&votes(),3,4),Err(GuestPackageError::Malformed));
    }
    #[test]fn duplicate_and_fake_votes_do_not_count(){
        let v=votes();let mut duplicate=v;duplicate[1]=v[0];
        assert_eq!(check(&duplicate,2,4),Err(GuestPackageError::Malformed));
        let mut bad=v;bad[1].signature[0]=0;
        assert_eq!(check(&bad,2,4),Err(GuestPackageError::Unsigned));
    }
    #[test]fn manifest_and_architecture_floor_are_mandatory(){
        assert_eq!(check(&votes(),2,3),Err(GuestPackageError::Downgrade));
        assert_eq!(check(&votes(),0,4),Err(GuestPackageError::Malformed));
        assert_eq!(verify_manifest_quorum(&TestOnlyVerifier,&roots(),&votes(),2,
            b"approved",4,0x1234,0x8664,4),
            Err(GuestPackageError::WrongArchitecture));
    }
}
