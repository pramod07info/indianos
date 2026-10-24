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
