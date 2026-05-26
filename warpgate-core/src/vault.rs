use std::env;
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use data_encoding::{BASE64, HEXLOWER};
use rand::RngExt;
use sea_orm::ActiveValue::Set;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, QueryFilter, QueryOrder, TransactionTrait,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;
use warpgate_common::helpers::rng::get_crypto_rng;
use warpgate_common::{LocalVaultConfig, Secret, WarpgateConfig, WarpgateError};
use warpgate_db_entities::{VaultAccessEvent, VaultItem};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("vault local backend is disabled")]
    Disabled,
    #[error("vault local backend is enabled but no master key is configured")]
    MissingMasterKey,
    #[error("vault local backend master key env var is not set: {0}")]
    MissingMasterKeyEnv(String),
    #[error("vault local backend master key must decode to 32 bytes, got {0}")]
    InvalidMasterKeyLength(usize),
    #[error("vault local backend key decode failed: {0}")]
    KeyDecode(String),
    #[error("vault payload decode failed: {0}")]
    PayloadDecode(String),
    #[error("vault crypto operation failed")]
    Crypto,
    #[error("vault secret is not valid UTF-8")]
    InvalidPlaintext,
    #[error("vault item not found: {0}")]
    ItemNotFound(Uuid),
    #[error("invalid vault input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

impl From<VaultError> for WarpgateError {
    fn from(error: VaultError) -> Self {
        Self::other(error)
    }
}

#[derive(Clone)]
pub struct VaultService {
    db: Arc<Mutex<DatabaseConnection>>,
    backend: Option<LocalVaultBackend>,
}

#[derive(Debug, Clone)]
pub struct CreateVaultSecret {
    pub name: String,
    pub kind: String,
    pub plaintext: Secret<String>,
    pub metadata: serde_json::Value,
    pub actor_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct UpdateVaultSecret {
    pub id: Uuid,
    pub plaintext: Secret<String>,
    pub metadata: Option<serde_json::Value>,
    pub actor_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct LocalVaultBackend {
    key: [u8; KEY_LEN],
    key_fingerprint: String,
}

#[derive(Debug, Clone)]
struct EncryptedSecret {
    ciphertext: String,
    nonce: String,
    aad: String,
}

impl VaultService {
    pub fn new(
        db: Arc<Mutex<DatabaseConnection>>,
        config: &WarpgateConfig,
    ) -> Result<Self, VaultError> {
        let backend = LocalVaultBackend::from_config(&config.store.vault.local)?;
        Ok(Self { db, backend })
    }

    pub const fn is_enabled(&self) -> bool {
        self.backend.is_some()
    }

    pub fn key_fingerprint(&self) -> Option<&str> {
        self.backend
            .as_ref()
            .map(|backend| backend.key_fingerprint.as_str())
    }

    pub async fn create_secret(
        &self,
        input: CreateVaultSecret,
    ) -> Result<VaultItem::Model, VaultError> {
        let backend = self.backend()?;
        validate_name_and_kind(&input.name, &input.kind)?;

        let id = Uuid::new_v4();
        let version = 1;
        let encrypted = backend.encrypt(id, version, &input.name, &input.kind, &input.plaintext)?;
        let now = OffsetDateTime::now_utc();

        let db = self.db.lock().await;
        let txn = db.begin().await?;

        let model = VaultItem::ActiveModel {
            id: Set(id),
            name: Set(input.name),
            kind: Set(input.kind),
            ciphertext: Set(encrypted.ciphertext),
            nonce: Set(encrypted.nonce),
            aad: Set(encrypted.aad),
            key_fingerprint: Set(backend.key_fingerprint.clone()),
            version: Set(version),
            metadata: Set(input.metadata),
            created: Set(now),
            updated: Set(now),
            created_by_user_id: Set(input.actor_user_id),
            deleted_at: Set(None),
        }
        .insert(&txn)
        .await?;

        insert_access_event(&txn, id, "create", input.actor_user_id, None, true, None).await?;
        txn.commit().await?;

        Ok(model)
    }

    pub async fn read_secret(
        &self,
        id: Uuid,
        actor_user_id: Option<Uuid>,
        session_id: Option<Uuid>,
    ) -> Result<Secret<String>, VaultError> {
        let backend = self.backend()?;
        let db = self.db.lock().await;

        let Some(item) = VaultItem::Entity::find_by_id(id)
            .filter(VaultItem::Column::DeletedAt.is_null())
            .one(&*db)
            .await?
        else {
            return Err(VaultError::ItemNotFound(id));
        };

        let result = backend.decrypt(&item);
        insert_access_event(
            &*db,
            id,
            "read",
            actor_user_id,
            session_id,
            result.is_ok(),
            result.as_ref().err().map(ToString::to_string),
        )
        .await?;

        result
    }

    pub async fn update_secret(
        &self,
        input: UpdateVaultSecret,
    ) -> Result<VaultItem::Model, VaultError> {
        let backend = self.backend()?;
        let db = self.db.lock().await;
        let txn = db.begin().await?;

        let Some(item) = VaultItem::Entity::find_by_id(input.id)
            .filter(VaultItem::Column::DeletedAt.is_null())
            .one(&txn)
            .await?
        else {
            return Err(VaultError::ItemNotFound(input.id));
        };

        let version = item.version + 1;
        let encrypted =
            backend.encrypt(item.id, version, &item.name, &item.kind, &input.plaintext)?;

        let mut active: VaultItem::ActiveModel = item.into_active_model();
        active.ciphertext = Set(encrypted.ciphertext);
        active.nonce = Set(encrypted.nonce);
        active.aad = Set(encrypted.aad);
        active.key_fingerprint = Set(backend.key_fingerprint.clone());
        active.version = Set(version);
        active.updated = Set(OffsetDateTime::now_utc());
        if let Some(metadata) = input.metadata {
            active.metadata = Set(metadata);
        }

        let model = active.update(&txn).await?;
        insert_access_event(
            &txn,
            input.id,
            "update",
            input.actor_user_id,
            None,
            true,
            None,
        )
        .await?;
        txn.commit().await?;

        Ok(model)
    }

    pub async fn delete_secret(
        &self,
        id: Uuid,
        actor_user_id: Option<Uuid>,
    ) -> Result<(), VaultError> {
        self.backend()?;
        let db = self.db.lock().await;
        let txn = db.begin().await?;

        let Some(item) = VaultItem::Entity::find_by_id(id)
            .filter(VaultItem::Column::DeletedAt.is_null())
            .one(&txn)
            .await?
        else {
            return Err(VaultError::ItemNotFound(id));
        };

        let mut active: VaultItem::ActiveModel = item.into_active_model();
        active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
        active.updated = Set(OffsetDateTime::now_utc());
        active.update(&txn).await?;

        insert_access_event(&txn, id, "delete", actor_user_id, None, true, None).await?;
        txn.commit().await?;

        Ok(())
    }

    pub async fn list_items(&self) -> Result<Vec<VaultItem::Model>, VaultError> {
        self.backend()?;
        let db = self.db.lock().await;
        Ok(VaultItem::Entity::find()
            .filter(VaultItem::Column::DeletedAt.is_null())
            .order_by_asc(VaultItem::Column::Name)
            .all(&*db)
            .await?)
    }

    pub async fn access_events(
        &self,
        item_id: Uuid,
    ) -> Result<Vec<VaultAccessEvent::Model>, VaultError> {
        self.backend()?;
        let db = self.db.lock().await;
        Ok(VaultAccessEvent::Entity::find()
            .filter(VaultAccessEvent::Column::VaultItemId.eq(item_id))
            .order_by_desc(VaultAccessEvent::Column::Timestamp)
            .all(&*db)
            .await?)
    }

    fn backend(&self) -> Result<&LocalVaultBackend, VaultError> {
        self.backend.as_ref().ok_or(VaultError::Disabled)
    }
}

impl LocalVaultBackend {
    fn from_config(config: &LocalVaultConfig) -> Result<Option<Self>, VaultError> {
        if !config.enable {
            return Ok(None);
        }

        let key_value = match (&config.master_key, &config.master_key_env) {
            (Some(key), _) => key.expose_secret().clone(),
            (None, Some(env_var)) => {
                env::var(env_var).map_err(|_| VaultError::MissingMasterKeyEnv(env_var.clone()))?
            }
            (None, None) => return Err(VaultError::MissingMasterKey),
        };

        Ok(Some(Self::from_key(decode_master_key(&key_value)?)))
    }

    fn from_key(key: [u8; KEY_LEN]) -> Self {
        let digest = Sha256::digest(key);
        Self {
            key,
            key_fingerprint: HEXLOWER.encode(&digest),
        }
    }

    fn encrypt(
        &self,
        id: Uuid,
        version: i32,
        name: &str,
        kind: &str,
        plaintext: &Secret<String>,
    ) -> Result<EncryptedSecret, VaultError> {
        let nonce_bytes = get_crypto_rng().random::<[u8; NONCE_LEN]>();
        let aad = aad_for_item(id, version, name, kind);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| VaultError::InvalidMasterKeyLength(KEY_LEN))?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext.expose_secret().as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| VaultError::Crypto)?;

        Ok(EncryptedSecret {
            ciphertext: BASE64.encode(&ciphertext),
            nonce: BASE64.encode(&nonce_bytes),
            aad,
        })
    }

    fn decrypt(&self, item: &VaultItem::Model) -> Result<Secret<String>, VaultError> {
        let nonce = decode_payload(&item.nonce)?;
        if nonce.len() != NONCE_LEN {
            return Err(VaultError::PayloadDecode(format!(
                "nonce must decode to {NONCE_LEN} bytes, got {}",
                nonce.len()
            )));
        }

        let ciphertext = decode_payload(&item.ciphertext)?;
        let aad = aad_for_item(item.id, item.version, &item.name, &item.kind);
        if item.aad != aad {
            return Err(VaultError::Crypto);
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| VaultError::InvalidMasterKeyLength(KEY_LEN))?;
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| VaultError::Crypto)?;

        Ok(Secret::new(
            String::from_utf8(plaintext).map_err(|_| VaultError::InvalidPlaintext)?,
        ))
    }
}

async fn insert_access_event<C>(
    db: &C,
    vault_item_id: Uuid,
    action: &str,
    actor_user_id: Option<Uuid>,
    session_id: Option<Uuid>,
    success: bool,
    reason: Option<String>,
) -> Result<(), VaultError>
where
    C: ConnectionTrait,
{
    VaultAccessEvent::ActiveModel {
        id: Set(Uuid::new_v4()),
        vault_item_id: Set(vault_item_id),
        action: Set(action.to_owned()),
        actor_user_id: Set(actor_user_id),
        session_id: Set(session_id),
        timestamp: Set(OffsetDateTime::now_utc()),
        success: Set(success),
        reason: Set(reason),
    }
    .insert(db)
    .await?;
    Ok(())
}

fn validate_name_and_kind(name: &str, kind: &str) -> Result<(), VaultError> {
    if name.trim().is_empty() {
        return Err(VaultError::InvalidInput(
            "vault item name cannot be empty".into(),
        ));
    }

    if kind.trim().is_empty() {
        return Err(VaultError::InvalidInput(
            "vault item kind cannot be empty".into(),
        ));
    }

    Ok(())
}

fn aad_for_item(id: Uuid, version: i32, name: &str, kind: &str) -> String {
    format!("warpgate:vault:v1:{id}:{version}:{name}:{kind}")
}

fn decode_master_key(value: &str) -> Result<[u8; KEY_LEN], VaultError> {
    let value = value.trim();
    let bytes = if let Some(value) = value.strip_prefix("hex:") {
        decode_hex(value)?
    } else if let Some(value) = value.strip_prefix("base64:") {
        BASE64
            .decode(value.as_bytes())
            .map_err(|error| VaultError::KeyDecode(error.to_string()))?
    } else if is_hex_key(value) {
        decode_hex(value)?
    } else {
        BASE64
            .decode(value.as_bytes())
            .map_err(|error| VaultError::KeyDecode(error.to_string()))?
    };

    if bytes.len() != KEY_LEN {
        return Err(VaultError::InvalidMasterKeyLength(bytes.len()));
    }

    let mut key = [0; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, VaultError> {
    HEXLOWER
        .decode(value.to_ascii_lowercase().as_bytes())
        .map_err(|error| VaultError::KeyDecode(error.to_string()))
}

fn decode_payload(value: &str) -> Result<Vec<u8>, VaultError> {
    BASE64
        .decode(value.as_bytes())
        .map_err(|error| VaultError::PayloadDecode(error.to_string()))
}

fn is_hex_key(value: &str) -> bool {
    value.len() == KEY_LEN * 2 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_hex_master_key() -> Result<(), VaultError> {
        let key = decode_master_key(
            "hex:000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )?;
        assert_eq!(key[0], 0);
        assert_eq!(key[31], 31);
        Ok(())
    }

    #[test]
    fn rejects_wrong_sized_master_key() {
        let err = decode_master_key("hex:000102").err();
        assert!(matches!(err, Some(VaultError::InvalidMasterKeyLength(3))));
    }

    #[test]
    fn encrypts_and_decrypts_with_authenticated_metadata() -> Result<(), VaultError> {
        let backend = LocalVaultBackend::from_key([7; KEY_LEN]);
        let id = Uuid::new_v4();
        let encrypted = backend.encrypt(
            id,
            1,
            "database/root",
            "password",
            &Secret::new("s3cr3t".into()),
        )?;

        assert_ne!(encrypted.ciphertext, "s3cr3t");

        let item = VaultItem::Model {
            id,
            name: "database/root".into(),
            kind: "password".into(),
            ciphertext: encrypted.ciphertext,
            nonce: encrypted.nonce,
            aad: encrypted.aad,
            key_fingerprint: backend.key_fingerprint.clone(),
            version: 1,
            metadata: json!({}),
            created: OffsetDateTime::now_utc(),
            updated: OffsetDateTime::now_utc(),
            created_by_user_id: None,
            deleted_at: None,
        };

        let plaintext = backend.decrypt(&item)?;
        assert_eq!(plaintext.expose_secret(), "s3cr3t");

        let mut tampered = item;
        tampered.name = "database/other".into();
        assert!(matches!(
            backend.decrypt(&tampered),
            Err(VaultError::Crypto)
        ));

        Ok(())
    }

    #[test]
    fn rejects_wrong_key() -> Result<(), VaultError> {
        let backend = LocalVaultBackend::from_key([7; KEY_LEN]);
        let wrong_backend = LocalVaultBackend::from_key([8; KEY_LEN]);
        let id = Uuid::new_v4();
        let encrypted = backend.encrypt(
            id,
            1,
            "database/root",
            "password",
            &Secret::new("s3cr3t".into()),
        )?;

        let item = VaultItem::Model {
            id,
            name: "database/root".into(),
            kind: "password".into(),
            ciphertext: encrypted.ciphertext,
            nonce: encrypted.nonce,
            aad: encrypted.aad,
            key_fingerprint: backend.key_fingerprint,
            version: 1,
            metadata: json!({}),
            created: OffsetDateTime::now_utc(),
            updated: OffsetDateTime::now_utc(),
            created_by_user_id: None,
            deleted_at: None,
        };

        assert!(matches!(
            wrong_backend.decrypt(&item),
            Err(VaultError::Crypto)
        ));

        Ok(())
    }
}
