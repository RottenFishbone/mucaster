pub mod error;

use crate::{cast, video_encoding::Chromecast};
use std::net::IpAddr;
use serde::{Serialize, Deserialize};
use tokio::{sync::oneshot, runtime::Handle};

pub type Error = error::ApiError;

/// `Request` are the used as the main wrapper for API interaction
/// They can be sent via channel and handled by the Api struct easily 
/// through `Api::handle_request()`.
/// All variants of `Request` accept a tokio `oneshot::Sender` as part of their parameters.
/// This is used to send JSON feedback to the API caller. If the reciever is dropped before
/// a response is sent, the feedback will simply be discarded without an error.
#[derive(Debug)]
#[allow(dead_code)]
pub enum Request {
    Put(PutType, oneshot::Sender<String>),
    Get(GetType, oneshot::Sender<String>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GetType {
    MediaStatus
}

/// PutTypes are used to determine what Put request is being called.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PutType {
    /// Used to transmit signals to the chromecast
    Control(CastSignal), 
    Discover,
}

/// CastSignals are used to send requests to the chromecast for playback
/// These are essentially the remote control for the chromecast.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CastSignal {
    /// CastSignal::Begin takes a u32 representing the index of the video file in the server's
    /// library. This will likely need to be retrieved with a Get before it can be determined.
    Begin(u32),
    Stop,
    Pause,
    Play,
    Seek(f32),
}

/// Api serves as an easily manipulated interface with a Caster.
/// The intended purpose is to streamline interaction between a client program
/// and this daemon.
pub struct Api {
    // TODO move caster to private, once appropriate control functions are in place
    pub caster: cast::Caster,
    current_chromecast: Option<(String, IpAddr)>,
    discovered_chromecasts: Vec<(String, IpAddr)>,
}

#[allow(dead_code)]
impl Api {
    pub fn new() -> Self {
        Self {  caster: cast::Caster::new(), 
                current_chromecast: None,
                discovered_chromecasts: Vec::new() }
    }
    
    /// Polls the network for mDNS devices to build a list of available chromecasts.
    /// The discovered devices are cached and can be returned with `get_discovered_chromecasts()`
    /// This function MUST be called on the tokio::runtimes' thread, otherwise, you will need to
    /// use the runtime's handle and replicate this function using that.
    /// # Returns
    /// `&Vec<(String, IpAddress)` - A vec containing all the found devices as (FriendlyName,
    /// IpAddress)
    /// `ApiError` - on failure
    pub fn discover_chromecasts(&mut self) -> Result<(), Error> {
        // Call find_chromecasts on tokio::runtime
        let (tx, mut rx) = oneshot::channel::<Result<Vec<(String, IpAddr)>, cast::Error>>();
        tokio::spawn( async move {
            tx.send(cast::find_chromecasts().await).unwrap();
        });
                
        // Wait for the thread to send the list of chromecasts
        let chromecasts;
        loop {
            if let Ok(msg) = rx.try_recv() {
                chromecasts = msg;
                break;
            }
        }
        
        // Either store the result or return the error
        match chromecasts {
            Ok(chromecasts) => self.discovered_chromecasts = chromecasts,
            Err(err) => return Err(err.into()),
        }

        Ok(())
    }

    /// Returns a reference the cached Vec holding all the previously discovered chromecasts.
    /// Note, there is no guarantee that any of the devices are still available.
    pub fn get_discovered_chromecasts(&self) -> &Vec<(String, IpAddr)> {
        &self.discovered_chromecasts
    }
    
    /// Sets the selected chromecast to the passed reference. Note, the device MUST be present
    /// in discovered chromecasts, otherwise this will return an error.
    pub fn select_chromecast(&mut self, device: &(String, IpAddr)) -> Result<(), Error> {
        if self.discovered_chromecasts.contains(&device) {
            self.current_chromecast = Some(device.clone());
            self.caster.set_device_addr(&device.1.to_string());
        }
        else{
            return Err(Error::ApiError("Device not found within discovered_chromecasts,try calling Api::discover_chromecasts() first.".into()));
        }
        
        Ok(())
    }

    /// Handles API requests from a client.
    pub fn handle_request(&mut self, request: Request) {
        match request {
            // Handle Put requests
            Request::Put(put, sender) => {
                match put {
                    // Forward CastSignal to handler
                    PutType::Control(signal) => self.handle_control_request(signal, sender),
                    // Perform mDNS discovery, this is blocking
                    PutType::Discover => self.discover_chromecasts().unwrap(),
                }
            }

            // Handle Get requests
            Request::Get(get, sender) => {
                match get {
                    GetType::MediaStatus => {
                        sender.send("Its doin stuff".into()).unwrap();
                    }
                }
            }
        }
    }
    
    /// Handles Request::Put(Control(CastSignal)) requests.
    /// These are essentially the remote control signals that handle video
    /// playback.
    /// # Parameters
    /// `signal: CastSignal` - The signal to handle, this determines what to tell the chromecast to
    /// do.
    /// `sender: Sender<String>` - The feedback to return to the client.
    // TODO Only reply to client after chromecast has reacted to signal. This allows for a client to determine when the chromecast has ACTUALLY enacted its request.
    fn handle_control_request(&self, signal: CastSignal, sender: oneshot::Sender<String>) {
        let _ = sender.send("Request recieved.".into());
        log::info!("[API] Request recieved: {:?}", signal);
        
        if !self.caster.is_streaming() {
            log::info!("[API] Failed request. Chromecast is not streaming.");
            return;
        }

        match signal {
            CastSignal::Begin(_) => todo!(),
            CastSignal::Stop => self.caster.stop().unwrap(),
            CastSignal::Pause => self.caster.pause().unwrap(),
            CastSignal::Play => self.caster.resume().unwrap(),
            CastSignal::Seek(seconds) => self.caster.seek(seconds).unwrap(),
        }
    }
}
