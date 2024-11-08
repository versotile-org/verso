use std::{env::current_exe, thread::sleep, time::Duration};

fn main() {
    let versoview_path = current_exe().unwrap().parent().unwrap().join("versoview");
    let controller = verso::VersoviewController::new(
        versoview_path,
        url::Url::parse("https://example.com").unwrap(),
    );
    controller
        .on_navigation_starting(|url| {
            dbg!(url);
            true
        })
        .unwrap();
    sleep(Duration::from_secs(10));
    dbg!(controller
        .navigate(url::Url::parse("https://docs.rs").unwrap())
        .unwrap());
    loop {
        sleep(Duration::MAX);
    }
}
