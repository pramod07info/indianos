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
    let anchor = anchors.iter().find(|key| key.id != 0 && key.id == key_id)
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
