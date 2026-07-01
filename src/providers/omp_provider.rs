use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::omp::{self, OmpRpcEvent, ResumeInput, TurnInput};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

pub struct OmpProvider {
    config: HarnessConfig,
    omp_binary: PathBuf,
    session: Mutex<Option<omp::OmpSession>>,
    working_dir: Mutex<Option<PathBuf>>,
}

impl OmpProvider {
    pub async fn new(config: &HarnessConfig, omp_binary: &Path) -> Result<Self, ProviderError> {
        Ok(Self {
            config: config.clone(),
            omp_binary: omp_binary.to_path_buf(),
            session: Mutex::new(None),
            working_dir: Mutex::new(None),
        })
    }

    fn get_models_config(&self) -> String {
        let cfg_dir = crate::providers::config::ProviderConfigDir::new(&PathBuf::from(
            std::env::var("OMPRINT_CONFIG")
                .unwrap_or_else(|_| format!("{}/.config/omprint", std::env::var("HOME").unwrap_or_else(|_| ".".into()))),
        ));
        if let Ok(pc) = cfg_dir.load_provider_config(&self.config.provider_config_ref) {
            pc.raw_snippet
        } else {
            String::new()
        }
    }
}

#[async_trait]
impl LlmProvider for OmpProvider {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError> {
        let models_config = self.get_models_config();
        if models_config.is_empty() {
            return Ok(vec!["default".to_string()]);
        }
        let mut models = Vec::new();
        for line in models_config.lines() {
            let trimmed = line.trim();
            if let Some(name) = trimmed.strip_prefix("model: ") {
                models.push(name.to_string());
            }
        }
        if models.is_empty() {
            models.push("default".to_string());
        }
        Ok(models)
    }

    async fn start(&mut self, working_dir: &Path) -> Result<(), ProviderError> {
        let cwd = working_dir.to_string_lossy().to_string();
        let env = HashMap::new();
        let session = omp::spawn_omp(
            self.omp_binary.to_str().unwrap_or("omp"),
            &cwd,
            env,
        )
        .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        *self.session.lock().unwrap() = Some(session);
        *self.working_dir.lock().unwrap() = Some(working_dir.to_path_buf());
        Ok(())
    }

    async fn start_turn(
        &self,
        input: TurnInput,
    ) -> Result<mpsc::Receiver<OmpRpcEvent>, ProviderError> {
        let mut session = self.session.lock().unwrap();
        let session = session
            .as_mut()
            .ok_or(ProviderError::NotStarted)?;
        let (tx, rx) = mpsc::channel(256);
        session.start_turn(&input, tx).map_err(|e| ProviderError::Protocol(e.to_string()))?;
        Ok(rx)
    }

    async fn resume_turn(
        &self,
        input: ResumeInput,
    ) -> Result<mpsc::Receiver<OmpRpcEvent>, ProviderError> {
        let mut session = self.session.lock().unwrap();
        let session = session
            .as_mut()
            .ok_or(ProviderError::NotStarted)?;
        let (tx, rx) = mpsc::channel(256);
        session.resume_turn(&input, tx).map_err(|e| ProviderError::Protocol(e.to_string()))?;
        Ok(rx)
    }

    async fn abort_turn(&self) -> Result<(), ProviderError> {
        let mut session = self.session.lock().unwrap();
        if let Some(s) = session.as_mut() {
            let _ = s.child.kill();
            let _ = s.child.wait();
        }
        *session = None;
        Ok(())
    }

    async fn one_shot_prompt(
        &self,
        prompt: &str,
        model: &str,
    ) -> Result<String, ProviderError> {
        let wd = self
            .working_dir
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let models_config = self.get_models_config();
        let input = TurnInput::new(
            prompt.to_string(),
            wd.to_string_lossy().to_string(),
            model.to_string(),
            self.config.effort.clone().unwrap_or_else(|| "balanced".to_string()),
            "auto".to_string(),
            vec![],
            models_config,
        );
        let mut session = omp::spawn_omp(
            self.omp_binary.to_str().unwrap_or("omp"),
            &wd.to_string_lossy(),
            HashMap::new(),
        )
        .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        let (tx, mut rx) = mpsc::channel(256);
        session.start_turn(&input, tx).map_err(|e| ProviderError::Protocol(e.to_string()))?;
        let mut response = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                OmpRpcEvent::Text { text } => response.push_str(&text),
                OmpRpcEvent::TextChunk { delta } => response.push_str(&delta),
                OmpRpcEvent::Error { error } => {
                    tracing::warn!("omp one_shot_prompt error: {error}");
                    break;
                }
                OmpRpcEvent::Done(_) => break,
                _ => {}
            }
        }
        if response.is_empty() {
            return Err(ProviderError::Protocol("no response from omp".into()));
        }
        Ok(response)
    }

    async fn shutdown(&mut self) -> Result<bool, ProviderError> {
        let mut session = self.session.lock().unwrap();
        if session.is_some() {
            let _ = session.as_mut().map(|s| {
                let _ = s.child.kill();
                let _ = s.child.wait();
            });
            *session = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
