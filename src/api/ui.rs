use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist"]
struct AdminUiAssets;

pub async fn serve_admin_ui(request: Request<Body>) -> Response<Body> {
    let asset_path = request
        .uri()
        .path()
        .trim_start_matches("/admin")
        .trim_start_matches('/')
        .split('?')
        .next()
        .filter(|path| !path.is_empty())
        .unwrap_or("index.html");

    if let Some(response) = embedded_asset_response(asset_path) {
        return response;
    }

    if asset_path_has_extension(asset_path) {
        return StatusCode::NOT_FOUND.into_response();
    }

    embedded_asset_response("index.html").unwrap_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "embedded admin UI index.html not found",
        )
            .into_response()
    })
}

fn embedded_asset_response(path: &str) -> Option<Response<Body>> {
    let asset = AdminUiAssets::get(path)?;
    let content_type = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    Some(
        Response::builder()
            .header(header::CONTENT_TYPE, content_type.as_ref())
            .header(header::CACHE_CONTROL, cache_control)
            .body(Body::from(asset.data.into_owned()))
            .expect("embedded asset response should be valid"),
    )
}

fn asset_path_has_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::asset_path_has_extension;

    #[test]
    fn detects_asset_paths_with_extensions() {
        assert!(asset_path_has_extension("assets/index.js"));
        assert!(asset_path_has_extension("favicon.ico"));
        assert!(!asset_path_has_extension("rules"));
        assert!(!asset_path_has_extension("upstreams/edit"));
    }
}
