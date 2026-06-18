use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::path::Path;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFact {
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MemorySearchResult {
    pub chunk: MemoryChunk,
    pub similarity: f32,
}

pub struct EmbeddingEngine;

impl EmbeddingEngine {
    pub fn new() -> Self { Self }

    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0_f32; 384];
        for c in text.chars() {
            let byte = c as u8;
            for i in 0..384 {
                vec[i] += ((byte as f32) * (i as f32 + 1.0)).sin();
            }
        }
        let mut magnitude = 0.0_f32;
        for v in &vec { magnitude += v * v; }
        magnitude = magnitude.sqrt();
        if magnitude > 0.0 {
            for v in &mut vec { *v /= magnitude; }
        }
        vec
    }

    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() { return 0.0; }
        let mut dot = 0.0_f32;
        for i in 0..a.len() { dot += a[i] * b[i]; }
        dot.clamp(0.0, 1.0)
    }
}

pub struct MemoryStore {
    pool: Pool<Sqlite>,
    embedding_engine: EmbeddingEngine,
}

impl MemoryStore {
    pub async fn new(db_path: &Path) -> Result<Self> {
        let path_str = db_path.to_string_lossy();
        let db_url = if path_str == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            if !db_path.exists() {
                std::fs::File::create(db_path)?;
            }
            format!("sqlite://{}?mode=rwc", path_str)
        };

        let pool = SqlitePoolOptions::new().connect(&db_url).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memory_chunks (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL
            )",
        ).execute(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS key_facts (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        ).execute(&pool).await?;

        Ok(Self {
            pool,
            embedding_engine: EmbeddingEngine::new(),
        })
    }

    pub async fn store_chunk(
        &self,
        session_id: &str,
        content: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let embedding = self.embedding_engine.embed(content);
        let created_at = Utc::now();
        let bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        sqlx::query(
            "INSERT INTO memory_chunks
             (id, session_id, content, embedding, created_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(content)
        .bind(bytes)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    pub async fn search_similar(
        &self,
        query: &str,
        top_n: usize,
    ) -> Result<Vec<MemorySearchResult>> {
        let query_embedding = self.embedding_engine.embed(query);
        let rows = sqlx::query(
            "SELECT id, session_id, content, embedding, created_at
             FROM memory_chunks",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let id: String = row.try_get("id")?;
            let session_id: String = row.try_get("session_id")?;
            let content: String = row.try_get("content")?;
            let embedding_blob: Vec<u8> = row.try_get("embedding")?;
            let created_at_str: String = row.try_get("created_at")?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| anyhow!("Failed to parse date: {}", e))?
                .with_timezone(&Utc);

            let chunk_embedding: Vec<f32> = embedding_blob
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();

            let similarity = EmbeddingEngine::cosine_similarity(
                &query_embedding,
                &chunk_embedding,
            );

            results.push(MemorySearchResult {
                chunk: MemoryChunk {
                    id,
                    session_id,
                    content,
                    embedding: chunk_embedding,
                    created_at,
                },
                similarity,
            });
        }

        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_n);
        Ok(results)
    }

    pub async fn set_fact(&self, key: &str, value: &str) -> Result<()> {
        let updated_at = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO key_facts (key, value, updated_at)
             VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET
             value=excluded.value,
             updated_at=excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        info!("Key fact set: {}", key);
        Ok(())
    }

    pub async fn get_fact(&self, key: &str) -> Result<Option<String>> {
        let result = sqlx::query(
            "SELECT value FROM key_facts WHERE key = ?"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = result {
            Ok(Some(row.try_get("value")?))
        } else {
            Ok(None)
        }
    }

    pub async fn get_all_facts(&self) -> Result<Vec<KeyFact>> {
        let rows = sqlx::query(
            "SELECT key, value, updated_at FROM key_facts"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut facts = Vec::new();
        for row in rows {
            let updated_at_str: String = row.try_get("updated_at")?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map_err(|e| anyhow!("Failed to parse date: {}", e))?
                .with_timezone(&Utc);
            facts.push(KeyFact {
                key: row.try_get("key")?,
                value: row.try_get("value")?,
                updated_at,
            });
        }
        Ok(facts)
    }

    pub async fn delete_fact(&self, key: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM key_facts WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow!("Fact not found: {}", key));
        }
        Ok(())
    }
}

pub struct ContextInjector<'a> {
    memory: &'a MemoryStore,
}

impl<'a> ContextInjector<'a> {
    pub fn new(memory: &'a MemoryStore) -> Self { Self { memory } }

    pub async fn build_system_prompt(
        &self,
        base_prompt: &str,
        query: &str,
        top_n: usize,
    ) -> Result<String> {
        let facts = self.memory.get_all_facts().await?;
        let mut key_facts_section = String::from("KEY FACTS:\n");
        for fact in facts {
            key_facts_section
                .push_str(&format!("- {}: {}\n", fact.key, fact.value));
        }

        let search_results = self.memory.search_similar(query, top_n).await?;
        let mut memories_section = String::from("RELEVANT MEMORIES:\n");
        for res in search_results {
            if res.similarity > 0.1 {
                memories_section.push_str(&format!(
                    "- [{:.2}] {}\n",
                    res.similarity, res.chunk.content
                ));
            }
        }

        Ok(format!(
            "{}\n\n{}\n\n{}",
            base_prompt, key_facts_section, memories_section
        ))
    }
}
