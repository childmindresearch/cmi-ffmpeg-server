use axum::Router;

#[path = "routers/ffmpeg.rs"]
mod ffmpeg_router;

#[path = "routers/health.rs"]
mod health_router;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let app = Router::new()
        .merge(ffmpeg_router::init_router())
        .merge(health_router::init_router());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
