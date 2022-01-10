use crate::api;

use std::path::Path;
use tokio::sync::{
    oneshot,
    mpsc::{Receiver, Sender}
};
use warp::Filter;

/// Launches a warp server to host the web interface. This includes the webapp
/// and the api.
pub async fn host_api(port: u16, 
    shutdown_rx: oneshot::Receiver<()>,
    cast_tx: Sender<api::Request>) {

    /*
    // TODO conditionally host the webapp, this is not trivial afaik
    let routes = warp::get().and(
        warp::fs::dir("webapp")
            .or(api)
    );*/

    let tx_filter = warp::any().map(move || cast_tx.clone());
    let put_signals = warp::put()
        .and(warp::path("api"))
        .and(warp::path("pause"))
        .and(warp::path::end())
        .and(tx_filter)
        .and_then(put_cast_signal);

    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(put_signals)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}

async fn put_cast_signal(mut cast_tx: Sender<api::Request>) ->
    Result<impl warp::Reply, warp::Rejection> {
    
    // Send a pause signal to the caster thread
    cast_tx.send(
        api::Request::Cast(
            api::CastSignal::Pause)
        )
        .await.unwrap();

    // Respond to the PUT with success
    Ok(
        warp::reply::with_status(
            "Pause request sent.", 
            warp::http::StatusCode::ACCEPTED
        )
    )
}




/// Opens a warp server to host a media file at the specified path and port.
/// A shutdown reciever is used to close the media server gracefully when requested.
#[allow(dead_code)]
pub async fn host_media(file: &Path, port: u16, shutdown_rx: oneshot::Receiver<()>) {
    let route = warp::fs::file(file.to_path_buf());
    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}
