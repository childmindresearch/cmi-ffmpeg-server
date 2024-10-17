use std::io::prelude::Read;
use std::process::Command;

use axum::routing;
use axum::{
    extract::DefaultBodyLimit,
    http::{self, StatusCode},
    response::IntoResponse,
};
use axum_typed_multipart::{FieldData, TryFromMultipart, TypedMultipart};
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
}

async fn post_ffmpeg(
    TypedMultipart(UploadFileRequest {
        mut file,
        to: output_format,
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

    match run_ffmpeg(file_contents, file_extension, &output_format) {
        Ok(value) => Ok(value),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn run_ffmpeg(file: bytes::Bytes, from: &str, to: &str) -> Result<Vec<u8>, std::io::Error> {
    let input_file = tempfile::Builder::new()
        .suffix(&format!(".{}", from))
        .tempfile()?;

    let output_file = tempfile::Builder::new()
        .suffix(&format!(".{}", to))
        .tempfile()?;

    std::fs::write(input_file.path(), file)?;

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input_file.path())
        .arg("-f")
        .arg(to)
        .arg(output_file.path())
        .status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "FFmpeg conversion failed",
        ));
    }

    let mut output_bytes = Vec::new();
    let mut file_reader = std::fs::File::open(output_file.path())?;
    file_reader.read_to_end(&mut output_bytes)?;

    return Ok(output_bytes);
}

fn convert_file_to_bytes(file: &mut NamedTempFile) -> std::io::Result<bytes::Bytes> {
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(bytes::Bytes::from(buffer))
}
