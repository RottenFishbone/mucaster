use std::path::PathBuf;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

extern crate ffmpeg_next as ffmpeg; 

mod cast;
mod server;
mod media;
mod api;

#[tokio::main]
async fn main() {
    fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply().unwrap();
    
    // Spawn the casting thread
    // this will be where the API is interfaced
    let mut caster = cast::Caster::new();    
    let (cast_tx, mut cast_rx) = tokio::sync::mpsc::channel::<api::Request>(1024);

    // Spawn webapp/api server
    let handle = Handle::current();
    std::thread::spawn(move || {
        handle.spawn( async move {
            let (_shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            server::host_api(8008, shutdown_rx, cast_tx).await;
        });
    });
    
    // Discover chromecasts on network
    // TODO This should be called by the API handler and cached
    let chromecasts = cast::find_chromecasts().await.unwrap();
    // Spawn the media server on another thread
    // TODO This should be done dynamically by the API handler
    let handle = Handle::current();
    std::thread::spawn( move || {
        handle.spawn( async move {
            let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
            let path = PathBuf::from("sample.mp4");
            server::host_media(&path, 8009, rx).await;
        });
    });

    if let Some(chromecast) = chromecasts.first() {
        caster.set_device_addr(&chromecast.1.to_string());
        caster.begin_cast(8009).unwrap();
    }
    
    // API loop
    loop {
        // Player Signals
        let signal = cast_rx.recv().await.unwrap();
        log::info!("[API] Signal received: {:?}", signal);
        match signal {
            api::Request::Cast(cast_signal) => {
                // TODO error handling
                match cast_signal {
                    api::CastSignal::Pause => caster.pause().unwrap(),
                    api::CastSignal::Play => caster.resume().unwrap(),
                    api::CastSignal::Stop => caster.stop().unwrap(),
                    api::CastSignal::Seek(time) => caster.seek(time).unwrap(),
                    _ => {}
                }
            }
        }
    };
}
