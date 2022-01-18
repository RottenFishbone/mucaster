use crate::api;

use std::path::Path;
use tokio::sync::{ oneshot, mpsc };
use warp::Filter;

/// Convert a json input into a CastSignal
fn json_to_signal() -> impl Filter<Extract = (api::CastSignal,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024).and(warp::body::json())
}

/// Launches a warp server to host the web interface. This includes the webapp
/// and the api.
pub async fn host_api(port: u16, 
    shutdown_rx: oneshot::Receiver<()>,
    api_tx: mpsc::Sender<api::Request>) {
    
    let webapp = warp::get().and(
        warp::fs::dir("webapp/dist/mucast-frontend")  
    )
    .and(warp::path::end());

    let tx_filter = warp::any().map(move || api_tx.clone());
    let put_signals = warp::put()
        .and(warp::path("api"))
        .and(warp::path("cast-signal"))
        .and(warp::path::end())
        .and(json_to_signal())
        .and(tx_filter.clone())
        .and_then(put_cast_signal);

    let get_media_status = warp::get()
        .and(warp::path("api"))
        .and(warp::path("media-status"))
        .and(warp::path::end())
        .and(tx_filter.clone())
        .and_then(get_media_status);

    let route = warp::any().and(
        webapp
            .or(put_signals)
            .or(get_media_status)
    );

    let addr = ([0,0,0,0], port);
    let (_, server) = warp::serve(route)
        .bind_with_graceful_shutdown(addr, async {
            shutdown_rx.await.ok();
        });
    
    server.await;
}

async fn get_media_status(mut api_tx: mpsc::Sender<api::Request>)
    -> Result<impl warp::Reply, warp::Rejection> {
    

    let (req_tx, req_rx) = oneshot::channel::<String>();
    let request = api::Request::Get(api::GetType::MediaStatus, req_tx);
    api_tx.send( request ).await.unwrap();

    match await_api_response(req_rx) {
        Ok(resp) => Ok(warp::reply::json(&resp)),
        Err(_) => Err(warp::reject::reject()),
    }
}


/// Put request function to send a CastSignal request to the API
async fn put_cast_signal(
    signal: api::CastSignal,
    mut api_tx: mpsc::Sender<api::Request>) 
    -> Result<impl warp::Reply, warp::Rejection> {
    
    let (req_tx, req_rx) = oneshot::channel::<String>();
    // Send the requested signal to the caster thread
    let request = api::Request::Put( api::PutType::Control(signal), req_tx);
    api_tx.send( request ).await.unwrap();
    
    match await_api_response(req_rx) {
        Ok(resp) => Ok(warp::reply::with_status( resp, warp::http::StatusCode::OK )),
        Err(_) => Err(warp::reject::reject()),
    }
}

/// Spin and wait for a response from the passed reciever.
/// # Parameters
/// oneshot::Receiver<String> - A reciever, with the sender linked to the api::Request
/// # Returns
/// Result<String, String> - API response on success, "Failed to reach API" on failure. 
pub fn await_api_response(mut rx: oneshot::Receiver<String>) -> Result<String, String> {
    // TODO timeout error
    loop {
        match rx.try_recv() {
            Ok(resp) => {
                return Ok(resp.into());
            },
            Err(oneshot::error::TryRecvError::Closed) => {
                return Err("Failed to reach API".into());
            },
            _ => {},
        }
    }
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
