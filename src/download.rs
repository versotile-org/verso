use std::{fs::File, io::Write, str::FromStr, time::Duration};

use ipc_channel::ipc::IpcSender;
use mime::Mime;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    utils::content_disposition_parser::{DispositionType, parse_content_disposition},
    verso::VersoInternalMsg,
};

/// Download ID
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DownloadId(String);

impl DownloadId {
    /// Create a new download id
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl FromStr for DownloadId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

/// Download status
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadItem {
    id: DownloadId,
    filename: String,
    file_size: u64,
    url: String,
    /// The status of the download
    pub status: String,
    /// The progress of the download
    pub progress: f64,
    /// The time the download was created
    created_at: i64,
    /// Whether the download is stopped
    pub stopped: bool,
    abort_sender: Option<IpcSender<bool>>,
}

impl DownloadItem {
    /// Create a new download status
    pub fn new(url: String, file_name: String, abort_sender: Option<IpcSender<bool>>) -> Self {
        Self {
            id: DownloadId::new(),
            status: "Waiting".to_string(),
            url,
            filename: file_name,
            file_size: 0,
            progress: 0.0,
            created_at: chrono::Local::now().timestamp_millis(),
            stopped: false,
            abort_sender,
        }
    }

    /// Get the id
    pub fn id(&self) -> &DownloadId {
        &self.id
    }

    /// Abort the download
    pub fn abort(&mut self) {
        if let Some(sender) = self.abort_sender.take() {
            let _ = sender.send(true);
        }
    }

    /// Convert to json string for display on the downloads page
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn set_file_size(&mut self, file_size: u64) {
        self.file_size = file_size;
    }
}

/// Update download state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateDownloadState {
    /// The status of the download
    pub status: Option<String>,
    /// The progress of the download
    pub progress: Option<f64>,
    /// Whether the download is stopped
    pub stopped: Option<bool>,
}

// TODO: support `multipart/form-data`
/// Check if the URL should be downloaded.
/// Returns `true` if should download or `false` if should continue navigation.
pub(crate) async fn check_should_download(client: &Client, url: &Url) -> (bool, Option<Response>) {
    let Ok(resp) = client.get(url.clone()).send().await else {
        // Failed to load url, pass it to Servo
        return (false, None);
    };

    let content_disposition = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|content_disposition| content_disposition.to_str().ok())
        .and_then(parse_content_disposition);

    // check if content type should trigger download
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|content_type| content_type.to_str().ok())
        .and_then(|content_type| Mime::from_str(content_type).ok());

    // Download if content disposition is attachment or content type is image, text, application, etc.
    let should_download = content_disposition
        .is_some_and(|cd| cd.disposition == DispositionType::Attachment)
        || content_type.is_some_and(should_download_content_type);

    (should_download, Some(resp))
}

/// Check the content type should be downloaded.
/// This is based on what Servo can handle at `servo/components/script/dom/servoparser/mod.rs`.
fn should_download_content_type(content_type: Mime) -> bool {
    match (
        content_type.type_(),
        content_type.subtype(),
        content_type.suffix(),
    ) {
        (mime::IMAGE, _, _)
        | (mime::TEXT, mime::PLAIN, _)
        | (mime::TEXT, mime::HTML, _)
        | (mime::TEXT, mime::XML, _)
        | (mime::APPLICATION, mime::XML, _)
        | (mime::APPLICATION, mime::JSON, _) => false,
        (mime::APPLICATION, subtype, Some(mime::XML)) if subtype == "xhtml" => false,
        _ => true,
    }
}

// TODO: should bring cookies from the original request in Servo which is not implemented yet
/// Download the body of the response and write it to a file.
pub(crate) async fn download_body(
    url: Url,
    mut resp: reqwest::Response,
    verso_internal_sender: IpcSender<VersoInternalMsg>,
) {
    let filename = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|content_disposition| content_disposition.to_str().ok())
        .and_then(parse_content_disposition)
        .and_then(|content_disposition| content_disposition.filename())
        .unwrap_or_else(|| url.path_segments().unwrap().last().unwrap().to_string());
    let host = url.host().unwrap();

    // Ask if user wants to download the file
    if rfd::MessageDialogResult::No
        == rfd::AsyncMessageDialog::new()
            .set_buttons(rfd::MessageButtons::YesNo)
            .set_description(format!("{host} wants to download file: {filename}"))
            .set_title("Save File")
            .show()
            .await
    {
        return;
    }

    // Ask user for file path
    let Some(file_handle) = rfd::AsyncFileDialog::new()
        .set_file_name(&filename)
        .save_file()
        .await
    else {
        return;
    };

    // Handle the filepath. We create a temporary file with a timestamp suffix to avoid conflicts and mitigate the risk of out of disk space.
    let file_path = file_handle.path();
    let temp_file_path = file_path.with_file_name(format!(
        "{}_{}.verso.tmp",
        filename,
        chrono::Utc::now().timestamp()
    ));

    // Create a channel to abort the download. Abort the download if we receive `true` from the channel.
    let (abort_sender, abort_receiver) = ipc_channel::ipc::channel::<bool>().unwrap();

    /* -- START DOWNLOAD --*/
    let mut download = DownloadItem::new(url.to_string(), filename, Some(abort_sender));

    // Create a dummy file with a temporary name.
    let mut file = match File::create_new(&temp_file_path) {
        Ok(file) => file,
        Err(io_err) => {
            if io_err.kind() == std::io::ErrorKind::AlreadyExists {
                log::error!("[Download] Temporary file already exists");
            } else {
                log::error!("[Download] Failed to create temporary file");
            }

            download.status = "Error: Failed to create temporary file.".to_string();
            download.stopped = true;

            return;
        }
    };

    // Allocate dummy file with content length, to ensure there's enough space.
    // If we failed to get the content length, we'll just write to the file as we get chunks.
    let content_length = resp
        .headers()
        .get("Content-Length")
        .and_then(|ct| ct.to_str().ok())
        .and_then(|ct| u64::from_str(ct).ok());
    if let Some(file_size) = content_length {
        download.set_file_size(file_size);
        if file.set_len(file_size).is_err() {
            log::error!("[Download] Failed to allocate space for file");
            download.status = "Error: No space left on device.".to_string();
            download.stopped = true;
            return;
        }
    }

    /* -- START WRITING BODY DATA TO THE FILE --*/
    let download_id = download.id().clone();

    // Send a initial message to the main process to create a download status on the downloads page.
    let _ = verso_internal_sender.send(VersoInternalMsg::CreateDownload(download));

    let mut wrote_bytes: usize = 0;
    let mut last_update = tokio::time::Instant::now();

    send_update_to_verso(
        &verso_internal_sender,
        &download_id,
        Some("Downloading".to_string()),
        None,
        None,
    );

    // Read the response body in chunks and write to the file.
    while let Ok(chunk) = resp.chunk().await {
        if let Ok(abort) = abort_receiver.try_recv() {
            if abort {
                send_update_to_verso(
                    &verso_internal_sender,
                    &download_id,
                    Some("Cancelled".to_string()),
                    None,
                    Some(true),
                );
                return;
            }
        }

        if let Some(bytes) = chunk {
            if file.write_all(&bytes).is_err() {
                send_update_to_verso(
                    &verso_internal_sender,
                    &download_id,
                    Some("Error: Failed to write to file.".to_string()),
                    None,
                    Some(true),
                );
                return;
            }

            wrote_bytes += bytes.len();

            // Update the progress with throttling
            if last_update.elapsed() >= Duration::from_millis(500) {
                if let Some(file_size) = content_length {
                    send_update_to_verso(
                        &verso_internal_sender,
                        &download_id,
                        None,
                        Some(wrote_bytes as f64 / file_size as f64 * 100.0),
                        None,
                    );
                }
                last_update = tokio::time::Instant::now();
            }
        } else {
            file.flush().unwrap();

            // rename the dummy file to the original filename
            if std::fs::rename(temp_file_path, file_path).is_ok() {
                send_update_to_verso(
                    &verso_internal_sender,
                    &download_id,
                    Some("Finished".to_string()),
                    Some(100.0),
                    Some(true),
                );
            } else {
                log::error!("[Download] Failed to rename dummy file back to original filename");
                send_update_to_verso(
                    &verso_internal_sender,
                    &download_id,
                    Some("Error: Failed to rename temporary file.".to_string()),
                    None,
                    Some(true),
                );
            }

            return;
        }
    }
}

fn send_update_to_verso(
    verso_internal_sender: &IpcSender<VersoInternalMsg>,
    download_id: &DownloadId,
    status: Option<String>,
    progress: Option<f64>,
    stopped: Option<bool>,
) {
    let _ = verso_internal_sender.send(VersoInternalMsg::UpdateDownload(
        download_id.clone(),
        UpdateDownloadState {
            status,
            progress,
            stopped,
        },
    ));
}
