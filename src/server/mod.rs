use std::path::Path;
use tokio::sync::oneshot::Receiver;

pub async fn host_webapp(port: u16, shutdown_rx: Receiver<()>) {
    let route = warp::fs::dir("dist");
    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}

pub async fn host_media(file: &Path, port: u16, shutdown_rx: Receiver<()>) {
    let route = warp::fs::file(file.to_path_buf());
    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}
