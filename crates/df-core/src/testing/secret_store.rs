//! In-memory fake of [`SecretStore`].

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use secrecy::SecretString;

use crate::seams::{SecretError, SecretKeyRef, SecretStore};

#[derive(Default)]
struct SecretStoreState {
    calls: Vec<SecretKeyRef>,
    entries: HashMap<(String, String), Result<SecretString, SecretError>>,
    created: Vec<(SecretKeyRef, BTreeMap<String, String>)>,
    replace_error: Option<SecretError>,
}

/// In-memory fake [`SecretStore`] that records [`SecretKeyRef`] lookups and returns seeded secrets.
#[derive(Clone, Default)]
pub struct FakeSecretStore {
    state: Arc<Mutex<SecretStoreState>>,
}

impl FakeSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all [`SecretKeyRef`] lookups recorded so far.
    pub fn calls(&self) -> Vec<SecretKeyRef> {
        self.state.lock().expect("lock poisoned").calls.clone()
    }

    /// Returns the Secrets created by `get_or_create` with the labels they were created with.
    pub fn created(&self) -> Vec<(SecretKeyRef, BTreeMap<String, String>)> {
        self.state.lock().expect("lock poisoned").created.clone()
    }

    /// Seeds a secret value for `(secret_name, secret_key)`.
    pub fn insert(
        &self,
        secret_name: impl Into<String>,
        secret_key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.state.lock().expect("lock poisoned").entries.insert(
            (secret_name.into(), secret_key.into()),
            Ok(SecretString::from(value.into())),
        );
    }

    /// Seeds an error for `(secret_name, secret_key)`.
    pub fn insert_error(
        &self,
        secret_name: impl Into<String>,
        secret_key: impl Into<String>,
        err: SecretError,
    ) {
        self.state
            .lock()
            .expect("lock poisoned")
            .entries
            .insert((secret_name.into(), secret_key.into()), Err(err));
    }

    /// Makes every later [`SecretStore::replace`] fail with `err`; `None` lets it work again.
    pub fn fail_replace(&self, err: Option<SecretError>) {
        self.state.lock().expect("lock poisoned").replace_error = err;
    }

    /// Deletes the seeded or created entry for `(secret_name, secret_key)`.
    pub fn remove(&self, secret_name: &str, secret_key: &str) {
        self.state
            .lock()
            .expect("lock poisoned")
            .entries
            .remove(&(secret_name.to_string(), secret_key.to_string()));
    }
}

#[async_trait]
impl SecretStore for FakeSecretStore {
    async fn get(&self, r: &SecretKeyRef) -> Result<SecretString, SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(r.clone());
        match state.entries.get(&(r.name.clone(), r.key.clone())) {
            Some(Ok(secret)) => Ok(secret.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(SecretError::NotFound(r.clone())),
        }
    }

    async fn get_or_create(
        &self,
        r: &SecretKeyRef,
        labels: &BTreeMap<String, String>,
    ) -> Result<SecretString, SecretError> {
        let generated = format!(
            "generated-{}",
            self.state.lock().expect("lock poisoned").created.len()
        );
        self.get_or_create_with(r, labels, &SecretString::from(generated))
            .await
    }

    async fn get_or_create_with(
        &self,
        r: &SecretKeyRef,
        labels: &BTreeMap<String, String>,
        value: &SecretString,
    ) -> Result<SecretString, SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(r.clone());
        let key = (r.name.clone(), r.key.clone());
        if let Some(entry) = state.entries.get(&key) {
            return entry.clone();
        }
        // The Secret exists without the key: never patch it (as `K8sSecretStore`).
        if state.entries.keys().any(|(name, _key)| *name == r.name) {
            return Err(SecretError::NotFound(r.clone()));
        }
        state.entries.insert(key, Ok(value.clone()));
        state.created.push((r.clone(), labels.clone()));
        Ok(value.clone())
    }

    async fn replace(&self, r: &SecretKeyRef, value: &SecretString) -> Result<(), SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        if let Some(err) = state.replace_error.clone() {
            return Err(err);
        }
        let key = (r.name.clone(), r.key.clone());
        if !state.entries.contains_key(&key) {
            return Err(SecretError::NotFound(r.clone()));
        }
        state.entries.insert(key, Ok(value.clone()));
        Ok(())
    }
}
