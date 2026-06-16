use crate::domain::tofu::ports::{DatabasePort, DownloaderPort, DownloaderPullError};
use async_trait::async_trait;
use docker_credential::DockerCredential;
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, ParseError, Reference, manifest};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

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
        tokio::fs::write(&file_path, image_content).await?;

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
            Err(_) => RegistryAuth::Anonymous,
            Ok(DockerCredential::UsernamePassword(username, password)) => {
                RegistryAuth::Basic(username, password)
            }
            Ok(DockerCredential::IdentityToken(_)) => RegistryAuth::Anonymous,
        }
    }
}

#[derive(Error, Debug)]
pub enum ArtifactSourceParseError {
    #[error(transparent)]
    PathParseError(#[from] ParseError),
}

#[derive(Debug, PartialEq)]
pub enum ArtifactSource {
    Local(PathBuf),
    OCI(Reference),
}

impl ArtifactSource {
    pub fn parse(path: String) -> Result<Self, ArtifactSourceParseError> {
        if let Ok(reference) = path.parse::<Reference>() {
            return Ok(Self::OCI(reference));
        }

        let clean_path = path.trim_start_matches("file://");

        // check if it's a local path on the disk
        let local_path = Path::new(clean_path);

        Ok(Self::Local(local_path.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tofu::ports::MockDatabasePort;
    use std::env;
    use std::sync::Mutex;
    use temp_env::async_with_vars;
    use temp_env::with_var;
    use test_temp_dir::test_temp_dir;

    #[tokio::test]
    async fn test_pull() {
        let _ = async_with_vars([("DOCKER_CONFIG", None::<String>)], async {
            let dir = test_temp_dir!();
            let download_path = dir.as_path_untracked().to_path_buf();
            let captured_hash: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
            let hash_capture_clone = Arc::clone(&captured_hash);

            let mut db = MockDatabasePort::new();
            db.expect_retrieve()
                .times(1)
                .returning(|_| Box::pin(async { Ok(None) }));
            db.expect_save().times(1).returning(move |_, _, hash| {
                *hash_capture_clone.lock().unwrap() = Some(hash.clone());

                Box::pin(async { Ok(()) })
            });

            let downloader = Downloader::new(download_path.clone(), db);
            let result = downloader
                .pull(String::from(
                    "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0",
                ))
                .await
                .unwrap();
            let actual_hash = captured_hash.lock().unwrap().take().unwrap();
            let expected_path = download_path.join(format!("{}.wasm", actual_hash));
            assert_eq!(result, expected_path);

            let mut db = MockDatabasePort::new();
            db.expect_retrieve().times(1).returning(move |_| {
                let actual_hash_clone = actual_hash.clone();
                Box::pin(async move { Ok(Some(actual_hash_clone)) })
            });
            db.expect_save().times(0);
            let downloader = Downloader::new(download_path.clone(), db);
            let result = downloader
                .pull(String::from(
                    "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0",
                ))
                .await
                .unwrap();
            assert_eq!(result, expected_path);
        })
        .await;
    }

    #[test]
    fn test_artifact_source_local() {
        let result = ArtifactSource::parse("/home/user/artifact".to_string()).unwrap();
        assert_eq!(
            result,
            ArtifactSource::Local(PathBuf::from("/home/user/artifact"))
        );

        let result = ArtifactSource::parse("./artifact".to_string()).unwrap();
        assert_eq!(result, ArtifactSource::Local(PathBuf::from("./artifact")));

        let result = ArtifactSource::parse("file:///tmp/artifact".to_string()).unwrap();
        assert_eq!(
            result,
            ArtifactSource::Local(PathBuf::from("/tmp/artifact"))
        );

        let result = ArtifactSource::parse("../artifact".to_string()).unwrap();
        assert_eq!(result, ArtifactSource::Local(PathBuf::from("../artifact")));
    }

    #[test]
    fn test_artifact_source_oci() {
        let result = ArtifactSource::parse("foo:latest".to_string()).unwrap();
        let reference: Reference = "foo:latest".parse().unwrap();
        assert_eq!(result, ArtifactSource::OCI(reference));

        let result = ArtifactSource::parse(
            "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0".to_string(),
        )
        .unwrap();
        let reference: Reference = "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0"
            .parse()
            .unwrap();
        assert_eq!(result, ArtifactSource::OCI(reference));
    }

    #[test]
    fn test_build_auth() {
        let mut fixture_path = env::current_dir().unwrap();
        fixture_path.push("testdata");
        fixture_path.push("docker_config");

        let dir = test_temp_dir!();
        let download_path = dir.as_path_untracked().to_path_buf();
        let db = MockDatabasePort::new();
        let downloader = Downloader::new(download_path, db);

        let _ = with_var("DOCKER_CONFIG", Some(fixture_path.as_os_str()), || {
            let reference: Reference = "ghcr.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0"
                .parse()
                .unwrap();
            let auth = downloader.build_auth(&reference);
            assert!(matches!(auth, RegistryAuth::Basic(_, _)));

            let reference: Reference =
                "fake-registry.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0"
                    .parse()
                    .unwrap();
            let auth = downloader.build_auth(&reference);
            assert!(matches!(auth, RegistryAuth::Anonymous));

            let reference: Reference =
                "missing-registry.io/tmuntaner/tofuya/plugin-gitlab-states:0.1.0"
                    .parse()
                    .unwrap();
            let auth = downloader.build_auth(&reference);
            assert!(matches!(auth, RegistryAuth::Anonymous));
        });
    }
}
