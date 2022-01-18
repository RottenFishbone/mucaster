use std::path::PathBuf;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

extern crate ffmpeg_next as ffmpeg; 

mod cast;
mod server;
mod video_encoding;
mod api;

use api::Api;

#[tokio::main]
async fn main() {
    fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply().unwrap();
    
    // Spawn the casting thread
    // this will be where the API is interfaced
    let (cast_tx, mut cast_rx) = tokio::sync::mpsc::channel::<api::Request>(1024);

    // Spawn webapp/api server
    let handle = Handle::current();
    std::thread::spawn(move || {
        handle.spawn( async move {
            let (_shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            server::host_api(8008, shutdown_rx, cast_tx).await;
        });
    });

    let mut api = Api::new();
    api.discover_chromecasts().unwrap();
    let chromecasts = api.get_discovered_chromecasts().clone();
    if let Some(cast) = chromecasts.first() {
        api.select_chromecast(cast).unwrap();    
    }
    else {
        println!("No chromecasts found. Aborting.");
        return;
    }

    let handle = Handle::current();
    std::thread::spawn( move || {
        handle.spawn( async move {
            let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
            let path = PathBuf::from("sample.mp4");
            server::host_media(&path, 8009, rx).await;
        });
    });

    api.caster.begin_cast(8009).unwrap();
    
    // API loop
    loop {
        if let Some(request) = cast_rx.recv().await {
            api.handle_request(request);
        }
    };
}
