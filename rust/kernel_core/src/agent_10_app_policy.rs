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

/// Evaluate the root application and every declared dependency as a unit.
/// Only a caller-held consent bit and permission mask can authorize the plan.
pub fn approve_plan(candidates: &[Candidate], host: Architecture,
    granted: u32, consent: bool) -> Result<u32, InstallError>
{
    if candidates.is_empty() { return Err(InstallError::PermissionDenied); }
    let mut requested = 0u32;
    for &candidate in candidates {
        approve(candidate, host, granted, consent)?;
        requested |= candidate.requested_permissions;
    }
    Ok(requested)
}
#[cfg(test)]
mod dependency_tests {
    use super::*;
    fn candidate(permissions: u32) -> Candidate {
        Candidate { catalog_trusted: true, manifest_signed: true,
            architecture: Architecture::X86_64, requested_permissions: permissions }
    }
    #[test]
    fn returns_complete_preapproved_permission_union() {
        assert_eq!(approve_plan(&[candidate(1), candidate(2)],
            Architecture::X86_64, 3, true), Ok(3));
    }
    #[test]
    fn unapproved_dependency_blocks_entire_plan() {
        assert_eq!(approve_plan(&[candidate(1), candidate(4)],
            Architecture::X86_64, 3, true), Err(InstallError::PermissionDenied));
    }
    #[test]
    fn unsigned_or_empty_plan_does_not_install() {
        let mut unverified = candidate(1);
        unverified.manifest_signed = false;
        assert_eq!(approve_plan(&[candidate(1),unverified],Architecture::X86_64,1,true),
            Err(InstallError::Unsigned));
        assert_eq!(approve_plan(&[],Architecture::X86_64,0,true),
            Err(InstallError::PermissionDenied));
    }
}
