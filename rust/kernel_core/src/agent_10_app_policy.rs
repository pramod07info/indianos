//! Offline app-install policy decision. No download, install or privilege changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Architecture { X86_64, AArch64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub catalog_trusted: bool,
    pub manifest_signed: bool,
    pub architecture: Architecture,
    pub requested_permissions: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError { UntrustedCatalog, Unsigned, ArchitectureMismatch, PermissionDenied }

/// Explicit consent is mandatory even if no special permissions are requested.
pub fn approve(candidate: Candidate, host: Architecture, granted: u32, consent: bool)
    -> Result<(), InstallError>
{
    if !candidate.catalog_trusted { return Err(InstallError::UntrustedCatalog); }
    if !candidate.manifest_signed { return Err(InstallError::Unsigned); }
    if candidate.architecture != host { return Err(InstallError::ArchitectureMismatch); }
    if !consent || (candidate.requested_permissions & granted) != candidate.requested_permissions {
        return Err(InstallError::PermissionDenied);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn candidate() -> Candidate {
        Candidate { catalog_trusted: true, manifest_signed: true,
            architecture: Architecture::X86_64, requested_permissions: 0b011 }
    }
    #[test]
    fn authorized_install_policy_passes() {
        assert_eq!(approve(candidate(), Architecture::X86_64, 0b111, true), Ok(()));
    }
    #[test]
    fn denies_missing_signature_catalog_and_architecture() {
        let mut c = candidate();
        c.catalog_trusted = false;
        assert_eq!(approve(c, Architecture::X86_64, 7, true), Err(InstallError::UntrustedCatalog));
        c.catalog_trusted = true; c.manifest_signed = false;
        assert_eq!(approve(c, Architecture::X86_64, 7, true), Err(InstallError::Unsigned));
        c.manifest_signed = true;
        assert_eq!(approve(c, Architecture::AArch64, 7, true), Err(InstallError::ArchitectureMismatch));
    }
    #[test]
    fn permission_mask_and_explicit_consent_are_required() {
        assert_eq!(approve(candidate(), Architecture::X86_64, 0b001, true),
            Err(InstallError::PermissionDenied));
        assert_eq!(approve(candidate(), Architecture::X86_64, u32::MAX, false),
            Err(InstallError::PermissionDenied));
    }
}
