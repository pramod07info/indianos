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
