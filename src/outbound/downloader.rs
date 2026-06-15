use crate::domain::tofu::ports::{DatabasePort, DownloaderPort, DownloaderPullError};
use async_trait::async_trait;
use docker_credential::{CredentialRetrievalError, DockerCredential};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference, manifest};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct Downloader<DB>
where
    DB: DatabasePort + Send + Sync + 'static,
{
    storage_dir: PathBuf,
    db: Arc<DB>,
}

impl<DB> Downloader<DB>
where
    DB: DatabasePort + Send + Sync + 'static,
{
    pub fn new(storage_dir: PathBuf, database: DB) -> Self {
        Self {
            db: Arc::new(database),
            storage_dir,
        }
    }
}

#[async_trait]
impl<DB> DownloaderPort for Downloader<DB>
where
    DB: DatabasePort + Send + Sync + 'static,
{
    async fn pull(&self, path: String) -> Result<PathBuf, DownloaderPullError> {
        let source = ArtifactSource::parse(path.clone())?;

        match source {
            ArtifactSource::Local(path) => Ok(path),
            ArtifactSource::OCI(reference) => {
                let hash = self.db.retrieve(path).await?;

                // if hash exists in DB, see if the artifact really exists. If it does, return its path.
                if let Some(hash) = hash {
                    let file_path = self.hash_to_path(hash);
                    if fs::exists(&file_path)? {
                        return Ok(file_path);
                    }
                }

                // download the artifact if it doesn't exist yet.
                self.download(reference).await
            }
        }
    }
}

impl<DB> Downloader<DB>
where
    DB: DatabasePort + Send + Sync + 'static,
{
    async fn download(&self, reference: Reference) -> Result<PathBuf, DownloaderPullError> {
        let auth = self.build_auth(&reference);
        let client = Client::new(oci_client::client::ClientConfig {
            protocol: oci_client::client::ClientProtocol::Https,
            ..Default::default()
        });

        let image_content = client
            .pull(
                &reference,
                &auth,
                vec![manifest::WASM_LAYER_MEDIA_TYPE, "application/wasm"],
            )
            .await
            .expect("Cannot pull Wasm module")
            .layers
            .into_iter()
            .next()
            .map(|layer| layer.data)
            .expect("No data found");

        let mut hasher = Sha256::new();
        hasher.update(&image_content);
        let hash = hex::encode(hasher.finalize());

        let file_path = self.hash_to_path(hash.clone());
        if !file_path.exists() {
            tokio::fs::write(&file_path, image_content).await?;
        }

        self.db
            .save(file_path.display().to_string(), 0, hash)
            .await?;

        Ok(file_path)
    }

    fn hash_to_path(&self, hash: String) -> PathBuf {
        self.storage_dir.join(format!("{}.wasm", hash))
    }

    fn build_auth(&self, reference: &Reference) -> RegistryAuth {
        let server = reference
            .resolve_registry()
            .strip_suffix('/')
            .unwrap_or_else(|| reference.resolve_registry());

        match docker_credential::get_credential(server) {
            Err(CredentialRetrievalError::ConfigNotFound) => RegistryAuth::Anonymous,
            Err(CredentialRetrievalError::NoCredentialConfigured) => RegistryAuth::Anonymous,
            Err(_) => RegistryAuth::Anonymous,
            Ok(DockerCredential::UsernamePassword(username, password)) => {
                RegistryAuth::Basic(username, password)
            }
            Ok(DockerCredential::IdentityToken(_)) => RegistryAuth::Anonymous,
        }
    }
}

pub enum ArtifactSource {
    Local(PathBuf),
    OCI(Reference),
}

impl ArtifactSource {
    pub fn parse(path: String) -> Result<Self, DownloaderPullError> {
        let is_file_explicit_path = path.starts_with("/")
            || path.starts_with("./")
            || path.starts_with("../")
            || path.starts_with("file://")
            || path.contains("\\"); // windows

        if is_file_explicit_path {
            let clean_path = path.trim_start_matches("file://");

            return Ok(Self::Local(PathBuf::from(clean_path)));
        }

        // check if it's a local path on the disk
        let local_path = Path::new(path.as_str());
        if local_path.exists() {
            return Ok(Self::Local(local_path.to_path_buf()));
        }

        let reference: Reference = path.parse()?;

        Ok(Self::OCI(reference))
    }
}
