use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ControllerMessage {
    NavigateTo(url::Url),
}
