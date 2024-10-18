use std::fs::File;
use std::io::prelude::Read;
use std::io::{self, Cursor};
use std::process::Command;

use axum::routing;
use axum::{
    extract::DefaultBodyLimit,
    http::{self, StatusCode},
    response::IntoResponse,
};
use axum_typed_multipart::{FieldData, TryFromMultipart, TypedMultipart};
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Builder;
use tempfile::NamedTempFile;
use tracing::{error, info};

pub(crate) fn init_router() -> axum::Router {
    axum::Router::new()
        .route("/ffmpeg", routing::post(post_ffmpeg))
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024 * 2)) // 2 Gigabytes
}

#[derive(TryFromMultipart)]
struct UploadFileRequest {
    #[form_data(limit = "unlimited")]
    file: FieldData<NamedTempFile>,
    to: String,
    max_file_size: Option<usize>,
}

async fn post_ffmpeg(
    TypedMultipart(UploadFileRequest {
        mut file,
        to: output_format,
        max_file_size,
    }): TypedMultipart<UploadFileRequest>,
) -> Result<impl IntoResponse, http::StatusCode> {
    info!("Entering POST pandoc endpoint.");
    let file_contents =
        convert_file_to_bytes(&mut file.contents).map_err(|_| return StatusCode::BAD_REQUEST)?;
    let file_name = file.metadata.file_name.ok_or_else(|| {
        error!("Filename not found in the uploaded file.");
        http::StatusCode::BAD_REQUEST
    })?;
    let file_extension = file_name.split(".").last().ok_or_else(|| {
        error!("Filename has no extension.");
        http::StatusCode::BAD_REQUEST
    })?;

    let files = match run_ffmpeg(file_contents, file_extension, &output_format, max_file_size) {
        Ok(value) => Ok(value),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }?;

    let tar_file = match tar(files) {
        Ok(value) => Ok(value),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }?;

    match gzip(tar_file) {
        Ok(value) => Ok(value),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn run_ffmpeg(
    file: bytes::Bytes,
    from: &str,
    to: &str,
    max_file_size: Option<usize>,
) -> Result<Vec<NamedTempFile>, Box<dyn std::error::Error>> {
    let input_file = tempfile::Builder::new()
        .suffix(&format!(".{}", from))
        .tempfile()?;

    std::fs::write(input_file.path(), file)?;

    let duration = get_duration(&input_file)?;
    let mut current_duration = 0.0;
    let mut counter = 0;

    let mut outputs: Vec<NamedTempFile> = Vec::new();
    while current_duration < duration {
        outputs.push(
            tempfile::Builder::new()
                .prefix(&format!("{:08}_", counter))
                .suffix(&format!(".{}", to))
                .tempfile()?,
        );

        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(input_file.path())
            .arg("-ss")
            .arg(current_duration.to_string())
            .args(
                max_file_size
                    .map(|size| vec!["-fs".to_string(), size.to_string()])
                    .unwrap_or_else(Vec::new),
            )
            .arg("-f")
            .arg(to)
            .arg(outputs.last().unwrap().path())
            .status()?;

        if !status.success() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "FFmpeg conversion failed",
            )));
        }

        counter += 1;
        current_duration += get_duration(outputs.last().unwrap())?;
    }

    return Ok(outputs);
}

fn tar(files: Vec<NamedTempFile>) -> io::Result<NamedTempFile> {
    let mut tarball = NamedTempFile::new()?;
    {
        let mut tar_builder = Builder::new(&mut tarball);
        for file in files {
            let path = file.path();
            let filename = path.file_name().unwrap();
            let mut f = File::open(path)?;
            tar_builder.append_file(filename, &mut f)?;
        }
        tar_builder.finish()?;
    }

    Ok(tarball)
}

fn gzip(file: NamedTempFile) -> io::Result<Vec<u8>> {
    let mut buffer = Cursor::new(Vec::new());
    let mut encoder = GzEncoder::new(&mut buffer, Compression::default());

    io::copy(&mut File::open(file.path())?, &mut encoder)?;

    encoder.finish()?;
    Ok(buffer.into_inner())
}

fn convert_file_to_bytes(file: &mut NamedTempFile) -> std::io::Result<bytes::Bytes> {
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(bytes::Bytes::from(buffer))
}

fn get_duration(file: &NamedTempFile) -> Result<f64, Box<dyn std::error::Error>> {
    let input_file_str = file.path().to_str().ok_or_else(|| {
        error!("Could not find file path for temp file.");
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to get tempfile filepath.",
        )
    })?;

    let output = Command::new("ffprobe")
        .arg("-i")
        .arg(input_file_str)
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-v")
        .arg("quiet")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .output()
        .map_err(|e| format!("Failed to execute ffprobe: {}", e))?;

    if !output.status.success() {
        return Err("ffprobe command failed".into());
    }

    let output_str = std::str::from_utf8(&output.stdout)
        .map_err(|e| format!("Invalid UTF-8 output from ffprobe: {}", e))?;

    let duration_str = output_str
        .split('.')
        .next()
        .ok_or_else(|| "Failed to parse duration".to_string())?;

    let duration: f64 = duration_str
        .trim()
        .parse()
        .map_err(|e| format!("Failed to convert duration to float: {}", e))?;

    Ok(duration)
}
