use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use secrecy::{ExposeSecret, SecretVec};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Pool, Sqlite,
};
use std::path::Path;

pub struct Vault {
    pool: Pool<Sqlite>,
    vault_key: SecretVec<u8>,
}

impl Vault {
    pub async fn new(db_path: &Path, master_password: &str) -> Result<Vault> {
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new().connect_with(options).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vault (
                name TEXT PRIMARY KEY,
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS vault_meta (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        let salt_row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT value FROM vault_meta WHERE key = 'salt'")
                .fetch_optional(&pool)
                .await?;

        let salt = match salt_row {
            Some((existing_salt,)) => existing_salt,
            None => {
                let mut new_salt = vec![0u8; 16];
                OsRng.fill_bytes(&mut new_salt);
                sqlx::query(
                    "INSERT INTO vault_meta (key, value) VALUES ('salt', ?)"
                )
                .bind(&new_salt)
                .execute(&pool)
                .await?;
                new_salt
            }
        };

        let params = Params::new(65536, 3, 1, Some(32))
            .map_err(|_| anyhow!("Failed to initialize Argon2 parameters"))?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let mut key_bytes = vec![0u8; 32];
        argon2
            .hash_password_into(master_password.as_bytes(), &salt, &mut key_bytes)
            .map_err(|_| anyhow!("Failed to derive vault key"))?;

        Ok(Vault {
            pool,
            vault_key: SecretVec::new(key_bytes),
        })
    }

    pub async fn store_key(&self, name: &str, key_bytes: &[u8]) -> Result<()> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new(
            Key::<Aes256Gcm>::from_slice(self.vault_key.expose_secret())
        );

        let ciphertext = cipher
            .encrypt(nonce, key_bytes)
            .map_err(|_| anyhow!("Failed to encrypt key payload"))?;

        sqlx::query(
            "INSERT INTO vault (name, nonce, ciphertext)
             VALUES (?, ?, ?)
             ON CONFLICT(name) DO UPDATE SET
             nonce = excluded.nonce,
             ciphertext = excluded.ciphertext",
        )
        .bind(name)
        .bind(&nonce_bytes[..])
        .bind(ciphertext)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_key(&self, name: &str) -> Result<SecretVec<u8>> {
        let row: (Vec<u8>, Vec<u8>) =
            sqlx::query_as("SELECT nonce, ciphertext FROM vault WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| anyhow!("Key not found in vault"))?;

        let nonce = Nonce::from_slice(&row.0);
        let cipher = Aes256Gcm::new(
            Key::<Aes256Gcm>::from_slice(self.vault_key.expose_secret())
        );

        let plaintext = cipher
            .decrypt(nonce, row.1.as_ref())
            .map_err(|_| anyhow!("Failed to decrypt key payload"))?;

        Ok(SecretVec::new(plaintext))
    }

    pub async fn delete_key(&self, name: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM vault WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow!("Key not found in vault"));
        }
        Ok(())
    }

    pub async fn list_keys(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM vault")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|(name,)| name).collect())
    }
}
