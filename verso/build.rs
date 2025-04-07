use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const VERSO_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    println!("cargo:rerun-if-env-changed=PRE_BUILT_VERSOVIEW");
    println!("cargo:rerun-if-env-changed=VERSO_ARCHIVE");

    if let Ok(pre_built_versoview_env) = env::var("PRE_BUILT_VERSOVIEW") {
        let output_directory = match pre_built_versoview_env.as_str() {
            "true" => None,
            _ => Some(PathBuf::from(pre_built_versoview_env)),
        };
        download_and_extract_verso(output_directory).unwrap();
    };
}

fn decompress_archive(
    archive: &Path,
    output_directory: Option<PathBuf>,
) -> Result<(), std::io::Error> {
    let output_directory = output_directory.unwrap_or_else(|| {
        let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
        // Not ideal, but there's no good way of getting the target directory
        out_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    if Command::new("tar")
        .current_dir(output_directory)
        .arg("-xf")
        .arg(archive)
        .status()?
        .success()
    {
        Ok(())
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }
}

fn download_archive(base_url: &str) -> Result<PathBuf, std::io::Error> {
    let target = env::var("TARGET").unwrap();
    let archive_path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("verso.tar.gz");
    if !archive_path.exists() {
        let download_url =
            format!("{base_url}/download/versoview-v{VERSO_VERSION}/verso-{target}.tar.gz");
        let curl_start = Instant::now();
        println!("Try downloading versoview from {download_url}");
        if !Command::new("curl")
            .arg("-L")
            .arg("-f")
            .arg("-s")
            .arg("-o")
            .arg(&archive_path)
            .arg(download_url)
            .status()?
            .success()
        {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        println!(
            "Successfully downloaded versoview archive in {} ms",
            curl_start.elapsed().as_millis()
        );
    }

    Ok(archive_path)
}

fn download_and_extract_verso(output_directory: Option<PathBuf>) -> Result<(), std::io::Error> {
    if let Ok(archive) = env::var("VERSO_ARCHIVE") {
        // If the archive variable is present, assume it's a URL base to download from.
        let archive = download_archive(&archive).unwrap_or(PathBuf::from(archive));
        // Panic directly since the archive is specified manually.
        decompress_archive(&archive, output_directory).unwrap();
    } else {
        let archive =
            download_archive("https://github.com/versotile-org/versoview-release/releases")?;
        decompress_archive(&archive, output_directory)?;
    };

    Ok(())
}
