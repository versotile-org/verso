use std::{path::PathBuf, thread::sleep, time::Duration};

fn main() {
    let controller = verso::VersoviewController::new(
        PathBuf::from("target/debug/versoview"),
        url::Url::parse("https://example.com").unwrap(),
    );
    sleep(Duration::from_secs(10));
    dbg!(controller
        .navigate(url::Url::parse("https://docs.rs").unwrap())
        .unwrap());
    loop {
        sleep(Duration::from_secs(10));
    }
}
