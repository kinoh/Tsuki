use crate::db::{Db, DbResult};
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ModuleDefinition {
  pub name: String,
  pub instructions: String,
  pub enabled: bool,
}

impl ModuleDefinition {
  pub fn new(name: impl Into<String>, instructions: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      instructions: instructions.into(),
      enabled: true,
    }
  }
}

#[derive(Clone)]
pub struct ModuleRegistry {
  db: Arc<Db>,
}

impl ModuleRegistry {
  pub fn new(db: Arc<Db>) -> Self {
    Self { db }
  }

  pub async fn ensure_defaults(&self, defaults: Vec<ModuleDefinition>) -> DbResult<()> {
    for module in defaults {
      self.db
        .upsert_module(&module.name, &module.instructions, module.enabled)
        .await?;
    }
    Ok(())
  }

  pub async fn upsert(
    &self,
    name: &str,
    instructions: &str,
    enabled: bool,
  ) -> DbResult<()> {
    self.db.upsert_module(name, instructions, enabled).await
  }
}

#[async_trait]
pub trait ModuleRegistryReader: Send + Sync {
  async fn list_active(&self) -> DbResult<Vec<ModuleDefinition>>;
}

#[async_trait]
impl ModuleRegistryReader for ModuleRegistry {
  async fn list_active(&self) -> DbResult<Vec<ModuleDefinition>> {
    let rows = self.db.list_active_modules().await?;
    Ok(
      rows
        .into_iter()
        .map(|(name, instructions, enabled)| ModuleDefinition {
          name,
          instructions,
          enabled,
        })
        .collect(),
    )
  }
}
