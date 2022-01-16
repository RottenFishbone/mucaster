use crate::api;

use std::path::Path;
use tokio::sync::{ oneshot, mpsc };
use warp::Filter;

fn json_to_signal() -> impl Filter<Extract = (api::CastSignal,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024).and(warp::body::json())
}

/// Launches a warp server to host the web interface. This includes the webapp
/// and the api.
pub async fn host_api(port: u16, 
    shutdown_rx: oneshot::Receiver<()>,
    cast_tx: mpsc::Sender<api::Request>) {
    
    let webapp = warp::get().and(
        warp::fs::dir("webapp/dist/mucast-frontend")  
    )
    .and(warp::path::end());

    let tx_filter = warp::any().map(move || cast_tx.clone());
    let put_signals = warp::put()
        .and(warp::path("api"))
        .and(warp::path("cast-signal"))
        .and(warp::path::end())
        .and(json_to_signal())
        .and(tx_filter)
        .and_then(put_cast_signal);

    
    let route = warp::any().and(
        webapp
            .or(put_signals)
    );

    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}

async fn put_cast_signal(
    signal: api::CastSignal,
    mut cast_tx: mpsc::Sender<api::Request>) ->
    Result<impl warp::Reply, warp::Rejection> {
    
    // Send the requested signal to the caster thread
    cast_tx.send( api::Request::Cast(signal) ).await.unwrap();

    // Respond to the PUT with success
    Ok(
        warp::reply::with_status(
            "Signal forwarded to chromecast.",
            warp::http::StatusCode::OK
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
