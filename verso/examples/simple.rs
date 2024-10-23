use std::{path::PathBuf, thread::sleep, time::Duration};
use versoview_messages::ControllerMessage;

fn main() {
    let sender = verso::run_versoview(
        PathBuf::from("target/debug/versoview"),
        url::Url::parse("https://example.com").unwrap(),
    );
    sleep(Duration::from_secs(10));
    println!("Sending NavigateTo https://docs.rs");
    sender
        .send(ControllerMessage::NavigateTo(
            url::Url::parse("https://docs.rs").unwrap(),
        ))
        .unwrap();
    loop {
        sleep(Duration::from_secs(10));
    }
}
