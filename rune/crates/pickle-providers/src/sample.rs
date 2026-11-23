use async_trait::async_trait;
use pickle_core::{Generation, GenerationPrompt, GenerativeProvider, PickleError, PipelineEvent};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::transport::{check_cancel, error};

/// Fixed network-free demonstration. Never pretend this provider explains arbitrary text.
#[derive(Default, Clone)]
pub struct SampleProvider;

impl SampleProvider {
    pub const PASSAGE: &'static str =
        "The treatment may reduce symptoms in some patients, but the evidence remains limited.";
    pub const ANSWER: &'static str = "The treatment **might help some people** feel better, but there is not enough evidence to be sure.\n\nThis is a fixed demonstration. Choose Local or Online AI to explore your own passages.";
}

#[async_trait]
impl GenerativeProvider for SampleProvider {
    fn is_remote(&self) -> bool {
        false
    }
    fn permits_cloud_checks(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        cancel: CancellationToken,
    ) -> Result<Generation, PickleError> {
        check_cancel(&cancel)?;
        if prompt.structured {
            return Err(error(
                "The fixed demonstration supports prose only. Connect a provider for charts.",
            ));
        }
        let _ = events.send(PipelineEvent::Draft(Self::ANSWER.into()));
        check_cancel(&cancel)?;
        Ok(Generation::new(
            Self::ANSWER,
            "Offline sample · fixed response",
        ))
    }
}
