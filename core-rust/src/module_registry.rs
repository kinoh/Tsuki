use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
  modules: Arc<RwLock<HashMap<String, ModuleDefinition>>>,
}

impl ModuleRegistry {
  pub fn new(defaults: Vec<ModuleDefinition>) -> Self {
    let mut modules = HashMap::new();
    for module in defaults {
      modules.insert(module.name.clone(), module);
    }
    Self {
      modules: Arc::new(RwLock::new(modules)),
    }
  }

  pub fn add(&self, definition: ModuleDefinition) -> bool {
    if let Ok(mut modules) = self.modules.write() {
      let replaced = modules.insert(definition.name.clone(), definition);
      return replaced.is_some();
    }
    false
  }

  pub fn disable(&self, name: &str) -> bool {
    if let Ok(mut modules) = self.modules.write() {
      if let Some(module) = modules.get_mut(name) {
        module.enabled = false;
        return true;
      }
    }
    false
  }

  pub fn list_active(&self) -> Vec<ModuleDefinition> {
    self
      .modules
      .read()
      .map(|modules| {
        let mut list = modules
          .values()
          .filter(|module| module.enabled)
          .cloned()
          .collect::<Vec<_>>();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
      })
      .unwrap_or_default()
  }
}
