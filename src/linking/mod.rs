use crate::{
    linking::generator::get_file_types,
    lockfile::DownloadedGraph,
    scripts::{execute_script, ScriptName},
    source::{
        fs::{cas_path, store_in_cas},
        traits::PackageRef,
    },
    Project, LINK_LIB_NO_FILE_FOUND, PACKAGES_CONTAINER_NAME,
};
use fs_err::tokio as fs;
use futures::future::try_join_all;
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::spawn_blocking;

/// Generates linking modules for a project
pub mod generator;

async fn create_and_canonicalize<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    let p = path.as_ref();
    fs::create_dir_all(p).await?;
    p.canonicalize()
}

async fn write_cas(destination: PathBuf, cas_dir: &Path, contents: &str) -> std::io::Result<()> {
    let hash = store_in_cas(cas_dir, contents.as_bytes(), |_| async { Ok(()) }).await?;

    fs::hard_link(cas_path(&hash, cas_dir), destination).await
}

impl Project {
    /// Links the dependencies of the project
    pub async fn link_dependencies(
        &self,
        graph: &DownloadedGraph,
    ) -> Result<(), errors::LinkingError> {
        let manifest = self.deser_manifest().await?;
        let manifest_target_kind = manifest.target.kind();
        let roblox_sync_config_gen_script = manifest
            .scripts
            .get(&ScriptName::RobloxSyncConfigGenerator.to_string());

        let package_types = try_join_all(
            graph
                .iter()
                .map(|(name, versions)| async move {
                    Ok::<_, errors::LinkingError>((name, try_join_all(versions.iter().map(|(version_id, node)| async move {
                        let Some(lib_file) = node.target.lib_path() else {
                            return Ok((version_id, vec![]));
                        };

                        let container_folder = node.node.container_folder(
                            &self
                                .package_dir()
                                .join(manifest_target_kind.packages_folder(version_id.target()))
                                .join(PACKAGES_CONTAINER_NAME),
                            name,
                            version_id.version(),
                        );

                        let types = if lib_file.as_str() != LINK_LIB_NO_FILE_FOUND {
                            let lib_file = lib_file.to_path(&container_folder);

                            let contents = match fs::read_to_string(&lib_file).await {
                                Ok(contents) => contents,
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                                    return Err(errors::LinkingError::LibFileNotFound(
                                        lib_file.display().to_string(),
                                    ));
                                }
                                Err(e) => return Err(e.into()),
                            };

                            let types = match spawn_blocking(move || get_file_types(&contents)).await.unwrap() {
                                Ok(types) => types,
                                Err(e) => {
                                    return Err(errors::LinkingError::FullMoon(
                                        lib_file.display().to_string(),
                                        e,
                                    ))
                                }
                            };

                            log::debug!("{name}@{version_id} has {} exported types", types.len());

                            types
                        } else {
                            vec![]
                        };

                        if let Some(build_files) = Some(&node.target)
                            .filter(|_| !node.node.pkg_ref.like_wally())
                            .and_then(|t| t.build_files())
                        {
                            let Some(script_path) = roblox_sync_config_gen_script else {
                                log::warn!("not having a `{}` script in the manifest might cause issues with Roblox linking", ScriptName::RobloxSyncConfigGenerator);
                                return Ok((version_id, vec![]));
                            };

                            execute_script(
                                ScriptName::RobloxSyncConfigGenerator,
                                &script_path.to_path(self.package_dir()),
                                std::iter::once(container_folder.as_os_str())
                                    .chain(build_files.iter().map(OsStr::new)),
                                self,
                                false,
                            )
                                .map_err(|e| {
                                    errors::LinkingError::GenerateRobloxSyncConfig(
                                        container_folder.display().to_string(),
                                        e,
                                    )
                                })?;
                        }

                        Ok((version_id, types))
                    })).await?.into_iter().collect::<HashMap<_, _>>()))
                }
                )
        )
            .await?
            .into_iter()
            .collect::<HashMap<_, _>>();

        let manifest = Arc::new(manifest);
        let package_types = Arc::new(package_types);

        try_join_all(graph.iter().flat_map(|(name, versions)| {
            versions.iter().map(|(version_id, node)| {
                let name = name.clone();
                let manifest = manifest.clone();
                let package_types = package_types.clone();

                async move {
                    let (node_container_folder, node_packages_folder) = {
                        let base_folder = create_and_canonicalize(
                            self.package_dir()
                                .join(manifest_target_kind.packages_folder(version_id.target())),
                        )
                        .await?;
                        let packages_container_folder = base_folder.join(PACKAGES_CONTAINER_NAME);

                        let container_folder = node.node.container_folder(
                            &packages_container_folder,
                            &name,
                            version_id.version(),
                        );

                        if let Some((alias, _, _)) = &node.node.direct.as_ref() {
                            if let Some((lib_file, types)) =
                                node.target.lib_path().and_then(|lib_file| {
                                    package_types
                                        .get(&name)
                                        .and_then(|v| v.get(version_id))
                                        .map(|types| (lib_file, types))
                                })
                            {
                                write_cas(
                                    base_folder.join(format!("{alias}.luau")),
                                    self.cas_dir(),
                                    &generator::generate_lib_linking_module(
                                        &generator::get_lib_require_path(
                                            &node.target.kind(),
                                            &base_folder,
                                            lib_file,
                                            &container_folder,
                                            node.node.pkg_ref.use_new_structure(),
                                            &base_folder,
                                            container_folder.strip_prefix(&base_folder).unwrap(),
                                            &manifest,
                                        )?,
                                        types,
                                    ),
                                )
                                .await?;
                            };

                            if let Some(bin_file) = node.target.bin_path() {
                                write_cas(
                                    base_folder.join(format!("{alias}.bin.luau")),
                                    self.cas_dir(),
                                    &generator::generate_bin_linking_module(
                                        &container_folder,
                                        &generator::get_bin_require_path(
                                            &base_folder,
                                            bin_file,
                                            &container_folder,
                                        ),
                                    ),
                                )
                                .await?;
                            }
                        }

                        (container_folder, base_folder)
                    };

                    for (dependency_name, (dependency_version_id, dependency_alias)) in
                        &node.node.dependencies
                    {
                        let Some(dependency_node) = graph
                            .get(dependency_name)
                            .and_then(|v| v.get(dependency_version_id))
                        else {
                            return Err(errors::LinkingError::DependencyNotFound(
                                dependency_name.to_string(),
                                dependency_version_id.to_string(),
                            ));
                        };

                        let Some(lib_file) = dependency_node.target.lib_path() else {
                            continue;
                        };

                        let base_folder = create_and_canonicalize(
                            self.package_dir().join(
                                version_id
                                    .target()
                                    .packages_folder(dependency_version_id.target()),
                            ),
                        )
                        .await?;
                        let packages_container_folder = base_folder.join(PACKAGES_CONTAINER_NAME);

                        let container_folder = dependency_node.node.container_folder(
                            &packages_container_folder,
                            dependency_name,
                            dependency_version_id.version(),
                        );

                        let linker_folder = create_and_canonicalize(
                            node_container_folder.join(
                                node.node
                                    .base_folder(version_id, dependency_node.target.kind()),
                            ),
                        )
                        .await?;

                        write_cas(
                            linker_folder.join(format!("{dependency_alias}.luau")),
                            self.cas_dir(),
                            &generator::generate_lib_linking_module(
                                &generator::get_lib_require_path(
                                    &dependency_node.target.kind(),
                                    &linker_folder,
                                    lib_file,
                                    &container_folder,
                                    dependency_node.node.pkg_ref.use_new_structure(),
                                    &node_packages_folder,
                                    container_folder.strip_prefix(&base_folder).unwrap(),
                                    &manifest,
                                )?,
                                package_types
                                    .get(dependency_name)
                                    .and_then(|v| v.get(dependency_version_id))
                                    .unwrap(),
                            ),
                        )
                        .await?;
                    }

                    Ok(())
                }
            })
        }))
        .await
        .map(|_| ())
    }
}

/// Errors that can occur while linking dependencies
pub mod errors {
    use thiserror::Error;

    /// Errors that can occur while linking dependencies
    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum LinkingError {
        /// An error occurred while deserializing the project manifest
        #[error("error deserializing project manifest")]
        Manifest(#[from] crate::errors::ManifestReadError),

        /// An error occurred while interacting with the filesystem
        #[error("error interacting with filesystem")]
        Io(#[from] std::io::Error),

        /// A dependency was not found
        #[error("dependency not found: {0}@{1}")]
        DependencyNotFound(String, String),

        /// The library file was not found
        #[error("library file at {0} not found")]
        LibFileNotFound(String),

        /// An error occurred while parsing a Luau script
        #[error("error parsing Luau script at {0}")]
        FullMoon(String, Vec<full_moon::Error>),

        /// An error occurred while generating a Roblox sync config
        #[error("error generating roblox sync config for {0}")]
        GenerateRobloxSyncConfig(String, #[source] std::io::Error),

        /// An error occurred while getting the require path for a library
        #[error("error getting require path for library")]
        GetLibRequirePath(#[from] super::generator::errors::GetLibRequirePath),
    }
}
