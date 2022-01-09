use std::path::Path;
use tokio::sync::oneshot::Receiver;
use warp::Filter;

pub async fn host_api(port: u16, shutdown_rx: Receiver<()>) {

    // api serves as the main 
    let api = warp::path("api").and(
        warp::path("test").map(|| indoc::indoc!{"
        {
            'data': 'Api is working!'   
        }"})
    );
    
    let routes = warp::get().and(
        warp::fs::dir("webapp")
            .or(api)
    );


    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(routes)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}


#[allow(dead_code)]
pub async fn host_media(file: &Path, port: u16, shutdown_rx: Receiver<()>) {
    let route = warp::fs::file(file.to_path_buf());
    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}
