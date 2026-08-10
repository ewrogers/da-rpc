use super::{ApiError, ApiState, ErrorState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
};
use std::{fs, io, path::Path as FilePath};

const MAX_MAP_FILE_SIZE: u64 = 4 * 1024 * 1024;

#[utoipa::path(
    get,
    path = "/maps/{map_id}/download",
    params(("map_id" = u16, Path, description = "Dark Ages map ID")),
    responses(
        (status = 200, description = "Raw Dark Ages map file", content_type = "application/octet-stream", body = Vec<u8>),
        (status = 404, description = "The map file is unavailable", body = ErrorState),
        (status = 413, description = "The map file exceeds the response limit", body = ErrorState),
        (status = 500, description = "The map file could not be read", body = ErrorState)
    )
)]
pub(super) async fn download(
    State(state): State<ApiState>,
    Path(map_id): Path<u16>,
) -> Result<Response, ApiError> {
    let Some(directory) = state.maps_directory() else {
        return Err(map_not_found(map_id));
    };
    let result = tokio::task::spawn_blocking(move || read(&directory, map_id))
        .await
        .map_err(|_| map_read_failed(map_id))?;
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(MapReadError::NotFound) => return Err(map_not_found(map_id)),
        Err(MapReadError::TooLarge) => {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "map_file_too_large",
                format!("map {map_id} exceeds the 4 MiB download limit"),
                None,
            ));
        }
        Err(MapReadError::Io) => return Err(map_read_failed(map_id)),
    };
    let file_name = format!("lod{map_id}.map");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{file_name}\""),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .map_err(|_| map_read_failed(map_id))
}

enum MapReadError {
    NotFound,
    TooLarge,
    Io,
}

fn read(directory: &FilePath, map_id: u16) -> Result<Vec<u8>, MapReadError> {
    let path = directory.join(format!("lod{map_id}.map"));
    let metadata = fs::metadata(&path).map_err(classify_io)?;
    if !metadata.is_file() {
        return Err(MapReadError::NotFound);
    }
    if metadata.len() > MAX_MAP_FILE_SIZE {
        return Err(MapReadError::TooLarge);
    }
    let bytes = fs::read(path).map_err(classify_io)?;
    if bytes.len() as u64 > MAX_MAP_FILE_SIZE {
        return Err(MapReadError::TooLarge);
    }
    Ok(bytes)
}

fn classify_io(error: io::Error) -> MapReadError {
    if error.kind() == io::ErrorKind::NotFound {
        MapReadError::NotFound
    } else {
        MapReadError::Io
    }
}

fn map_not_found(map_id: u16) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "map_not_found",
        format!("map {map_id} is unavailable"),
        None,
    )
}

fn map_read_failed(map_id: u16) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "map_read_failed",
        format!("map {map_id} could not be read"),
        None,
    )
}
