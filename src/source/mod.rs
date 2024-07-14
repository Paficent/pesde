use crate::{manifest::DependencyType, names::PackageNames, Project};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{Debug, Display},
    path::Path,
};

pub mod pesde;

pub(crate) fn hash<S: std::hash::Hash>(struc: &S) -> String {
    use std::{collections::hash_map::DefaultHasher, hash::Hasher};

    let mut hasher = DefaultHasher::new();
    struc.hash(&mut hasher);
    hasher.finish().to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum DependencySpecifiers {
    Pesde(pesde::PesdeDependencySpecifier),
}
pub trait DependencySpecifier: Debug + Display {}
impl DependencySpecifier for DependencySpecifiers {}

impl Display for DependencySpecifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencySpecifiers::Pesde(specifier) => write!(f, "{}", specifier),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PackageRefs {
    Pesde(pesde::PesdePackageRef),
}
pub trait PackageRef: Debug {
    fn dependencies(&self) -> &BTreeMap<String, (DependencySpecifiers, DependencyType)>;
}
impl PackageRef for PackageRefs {
    fn dependencies(&self) -> &BTreeMap<String, (DependencySpecifiers, DependencyType)> {
        match self {
            PackageRefs::Pesde(pkg_ref) => pkg_ref.dependencies(),
        }
    }
}

pub type ResolveResult<Ref> = (PackageNames, BTreeMap<Version, Ref>);

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub enum PackageSources {
    Pesde(pesde::PesdePackageSource),
}
pub trait PackageSource: Debug {
    type Ref: PackageRef;
    type Specifier: DependencySpecifier;
    type RefreshError: std::error::Error;
    type ResolveError: std::error::Error;
    type DownloadError: std::error::Error;

    fn refresh(&self, _project: &Project) -> Result<(), Self::RefreshError> {
        Ok(())
    }

    fn resolve(
        &self,
        specifier: &Self::Specifier,
        project: &Project,
    ) -> Result<ResolveResult<Self::Ref>, Self::ResolveError>;

    fn download(
        &self,
        pkg_ref: &Self::Ref,
        destination: &Path,
        project: &Project,
    ) -> Result<(), Self::DownloadError>;
}
impl PackageSource for PackageSources {
    type Ref = PackageRefs;
    type Specifier = DependencySpecifiers;
    type RefreshError = errors::RefreshError;
    type ResolveError = errors::ResolveError;
    type DownloadError = errors::DownloadError;

    fn refresh(&self, project: &Project) -> Result<(), Self::RefreshError> {
        match self {
            PackageSources::Pesde(source) => source.refresh(project).map_err(Into::into),
        }
    }

    fn resolve(
        &self,
        specifier: &Self::Specifier,
        project: &Project,
    ) -> Result<ResolveResult<Self::Ref>, Self::ResolveError> {
        match (self, specifier) {
            (PackageSources::Pesde(source), DependencySpecifiers::Pesde(specifier)) => source
                .resolve(specifier, project)
                .map(|(name, results)| {
                    (
                        name,
                        results
                            .into_iter()
                            .map(|(version, pkg_ref)| (version, PackageRefs::Pesde(pkg_ref)))
                            .collect(),
                    )
                })
                .map_err(Into::into),

            _ => Err(errors::ResolveError::Mismatch),
        }
    }

    fn download(
        &self,
        pkg_ref: &Self::Ref,
        destination: &Path,
        project: &Project,
    ) -> Result<(), Self::DownloadError> {
        match (self, pkg_ref) {
            (PackageSources::Pesde(source), PackageRefs::Pesde(pkg_ref)) => source
                .download(pkg_ref, destination, project)
                .map_err(Into::into),

            _ => Err(errors::DownloadError::Mismatch),
        }
    }
}

pub mod errors {
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum RefreshError {
        #[error("error refreshing pesde package source")]
        Pesde(#[from] crate::source::pesde::errors::RefreshError),
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum ResolveError {
        #[error("mismatched dependency specifier for source")]
        Mismatch,

        #[error("error resolving pesde package")]
        Pesde(#[from] crate::source::pesde::errors::ResolveError),
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum DownloadError {
        #[error("mismatched package ref for source")]
        Mismatch,

        #[error("error downloading pesde package")]
        Pesde(#[from] crate::source::pesde::errors::DownloadError),
    }
}
