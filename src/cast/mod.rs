#![allow(dead_code, unused_variables)]
pub mod error;

use error::CastError;
use mdns::{Record, RecordKind};
use futures_util::{pin_mut, stream::StreamExt};
use regex::Regex;
use serde::{Serialize, ser::SerializeStruct};
use warp::hyper::{Client, body::HttpBody};
use std::{future, net::{IpAddr, UdpSocket}, sync::{mpsc::{Sender, TryRecvError}, Mutex, Arc}, thread, time::{SystemTime, Duration}};
use rust_cast::{CastDevice, ChannelMessage, channels::media::MediaResponse};
use rust_cast::channels::{
    heartbeat::HeartbeatResponse,
    media::{Media, StatusEntry, StreamType},
    receiver::CastDeviceApp,
};

pub type Error = error::CastError;

const DESTINATION_ID: &'static str = "receiver-0";
const SERVICE_NAME: &'static str = "_googlecast._tcp.local";
const TIMEOUT_SECONDS: u64 = 3;
const STATUS_UPDATE_INTERVAL: u128 = 500;

/// An enum containing useful playback info for the caster, can be serialized.
#[derive(Debug, Clone)]
pub enum MediaStatus {
    Active(StatusEntry),
    Inactive,
}
// Convert from rust-cast StatusEntry type into mucaster MediaStatus
impl From<StatusEntry> for MediaStatus {
    fn from(entry: StatusEntry) -> Self { MediaStatus::Active(entry) }
}
// Serializing is implemented to transmit playback data over HTTP
impl Serialize for MediaStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        let mut state;
        match self {
            MediaStatus::Inactive => {
                state = serializer.serialize_struct("status", 1).unwrap();
                state.serialize_field("playbackState", "Inactive").unwrap();
                state.end()
            }
            MediaStatus::Active(entry) => {    
                let mut num_fields = 1;
                if entry.current_time.is_some() { num_fields += 1; }
                if let Some(media) = &entry.media { 
                    if media.duration.is_some() { num_fields += 1; }
                }

                state = serializer.serialize_struct("status", num_fields).unwrap();
                state.serialize_field("playbackState", &entry.player_state.to_string()).unwrap();
                if let Some(media) = &entry.media {
                    state.serialize_field("videoLength", &media.duration.unwrap()).unwrap();
                }
                if let Some(time) = &entry.current_time {
                    state.serialize_field("currentTime", time).unwrap();
                }
                state.end()
            }
        }
    }
}

enum PlayerSignal {
    Play,
    Pause,
    Stop,
    Seek(f32),
}

pub struct Caster {
    device_addr: Option<String>,
    shutdown_tx: Option<Sender<()>>,
    pub status: Arc<Mutex<MediaStatus>>,
}
impl Drop for Caster {
    fn drop(&mut self) {
        self.close();
    }
}
impl Caster {
    pub fn new() -> Self {
        Self {
            device_addr: None,
            shutdown_tx: None,
            status: Arc::from(Mutex::from(MediaStatus::Inactive)),
        }
    }
    
    /// Check if the caster is linked to a chromecast and is actively playing
    /// content.
    /// # Returns
    /// `true` if the caster is currently streaming, else `false`. 
    /// Note, ended playback will return `false`.
    pub fn is_streaming(&self) -> bool {
        let is_active = match self.status.lock().unwrap().clone() {
            MediaStatus::Inactive => false,
            _ => true,
        };

        self.device_addr.is_some() && is_active
    }

    /// Set the target chromecast IP address to use.
    pub fn set_device_addr(&mut self, addr: &str) {
        self.device_addr = Some(addr.into());
    }

    /// Close the connection between the Caster and the Chromecast device, 
    /// if possible.  
    pub fn close(&mut self) {
        if self.is_streaming() {
            self.stop().unwrap();
        }
        // Send a shutdown signal to the keep-alive thread
        if let Some(sender) = &self.shutdown_tx {
            let _ = sender.send(());
            self.shutdown_tx = None;
        }
    }

    /// Open a new connection with the Chromecast. An event loop thread will be
    /// spawned to manage keep alive and poll for media status updates.
    pub fn begin_cast(&mut self, media_port: u16) -> Result<(), CastError> {
        // Ensure there is a device to cast to
        let addr = match &self.device_addr {
            Some(addr) => addr.clone(),
            None => {
                return Err(CastError::CasterError("No device address selected."));
            }
        };
        // Channel to kill casting
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        // Open a thread to handle recieve status updates
        let status_ref = self.status.clone();
        let mut last_media_status = SystemTime::now();
        let mut status_delay = 5000; 
        let handle = thread::spawn(move || {
            // Open the device connection
            let device = CastDevice::connect_without_host_verification(addr, 8009).unwrap();
            device.connection.connect(DESTINATION_ID).unwrap();
            log::info!("[Chromecast] Connected to device");

            // Launch the media player on the device
            let app = device.receiver.launch_app(
                &CastDeviceApp::DefaultMediaReceiver).unwrap();
            let transport_id = app.transport_id.to_string();
            let session_id = app.session_id.to_string();
            
            log::info!("[Chromecast] Launched media app.");

            // Connect to the app and begin playback
            let media_addr = format!("http://{}:{}", 
                get_local_ip().unwrap(), 
                media_port);
            
            device.connection.connect(&transport_id).unwrap();
            device.media.load(
                &transport_id, 
                &session_id, 
                &Media {
                    content_id: media_addr, 
                    content_type: "video/mp4".to_string(),
                    stream_type: StreamType::None,
                    duration: None,
                    metadata: None,
                },
            ).unwrap();

            log::info!("[Chromecast] Loaded media.");
            
            // Chromecast communication loop
            loop { 
                // Poll the shutdown reciever
                match shutdown_rx.try_recv() {
                    Ok(_) | Err(TryRecvError::Disconnected) => {
                        // Break the thread loop, closing the thread
                        log::info!("[Chromecast] Closing comm thread.");
                        return;                        
                    },
                    Err(TryRecvError::Empty) => {}
                }

                // Handle device communication
                // If this is not done often enough the connection will die
                // This blocking call is why media control is on a separate 
                // thread from status updates
                // TODO utilize rust-cast 1.6 thread_safe, where was that a year ago :P
                if let Some((ch_msg, msg)) = Caster::handle_device_status(&device){
                    log::info!("[Device Message] {}", &msg);
                }

                let millis_since_last = last_media_status
                    .elapsed().unwrap()
                    .as_millis();

                // Update media status if it hasn't recently
                if millis_since_last >= status_delay {
                    status_delay = STATUS_UPDATE_INTERVAL;
                    // Retrieve media status
                    let statuses = match device.media
                        .get_status(&transport_id, None) {
                            Ok(statuses) => statuses,
                            Err(err) => {
                                log::info!("[Chromecast] Error: {:?}", err);
                                continue;
                            },
                    };
                    // Map StatusEntry to MediaStatus enum
                    let status = match statuses.entries.first() {
                        Some(status) => MediaStatus::Active(status.clone()),
                        None => MediaStatus::Inactive
                    };
                    log::info!("[Chromecast] [Status] {:?}", &status);
                    *status_ref.lock().unwrap() = status;
                    last_media_status = SystemTime::now();
                }
                
            }
        });
        
        Ok(())
    }

    /// Block until device status is received.  
    /// The message is parsed into a string, and returned.  
    /// If the message was a Heartbeat, a pong will be returned to the 
    /// chromecast.
    /// ### Returns
    /// - On success: ***Some(Log message as String)***
    /// - On error: ***None***
    fn handle_device_status(device: &CastDevice) 
        -> Option<(ChannelMessage, String)> {
        match device.receive() {
            Ok(msg) => {
                let log_msg: String;
                match &msg {
                    ChannelMessage::Connection(resp) => {
                        return Some((msg.clone(), 
                            format!("[Device=>Connection] {:?}", resp)));
                    }
                    ChannelMessage::Media(resp) => {
                        Self::handle_media_status(resp);
                        return Some((msg.clone(), 
                            format!("[Device=>Media] {:?}", resp)));
                    }
                    ChannelMessage::Receiver(resp) => {
                        return Some((msg.clone(), 
                            format!("[Device=>Receiver] {:?}", resp)));
                    }
                    ChannelMessage::Raw(resp) => {
                        return Some((msg.clone(),
                            format!("[Device] Message could not 
                                            be parsed: {:?}", resp)));
                    }
                    ChannelMessage::Heartbeat(resp) => {
                        // Reply to ping with pong
                        if let HeartbeatResponse::Ping = resp {
                            device.heartbeat.pong().unwrap();
                            log::info!("[Heartbeat] Pong sent.");
                        }
                        return Some((msg.clone(),
                            (format!("[Heartbeat] {:?}", resp))));
                    }
                }
            },
            // Failed to receive message
            Err(err) => {
                log::error!("An error occured while recieving 
                            message from chromecast:\n{:?}", err);
                return None
            }
        }
    }
    
    // TODO this function can likely be deleted and device message media updates ignored
    fn handle_media_status(resp: &MediaResponse) {
        let status = match resp {
            MediaResponse::Status(status) => status.clone(),
            _=> {return;}
        };
    }

    /// Resumes playback on chromecast if it is paused.
    pub fn resume(&self) -> Result<(), CastError> {
        self.change_media_state(PlayerSignal::Play)?;
        Ok(())
    }
    
    /// Pauses playback on chromecast if it is playing.
    pub fn pause(&self) -> Result<(), CastError> {
        self.change_media_state(PlayerSignal::Pause)?;
        Ok(())
    }
    
    /// Stops playback and returns to the splashscreen
    pub fn stop(&self) -> Result<(), CastError> {
        self.change_media_state(PlayerSignal::Stop)?;
        Ok(())
    }

    /// Seek current playback to specified time.
    /// ### Arguments 
    /// * time - A float representing the time in seconds to
    ///     seek to.
    pub fn seek(&self, time: f32) -> Result<(), CastError> {
        self.change_media_state(PlayerSignal::Seek(time))?;
        Ok(())
    }

    /// Calls one of the functions that alter the play state
    /// on the current playback. 
    /// ### Arguments
    /// * state - A MediaState to apply to the current playback
    fn change_media_state(&self, state: PlayerSignal) -> Result<(),CastError> {
        // Open a new connection
        let device = self.connect()?;
        let status = device.receiver.get_status()?;
        let app = status.applications.first().unwrap();

        // Connect to application
        device.connection.connect(app.transport_id.to_string())?;

        let media_status = device.media
            .get_status(
                app.transport_id.as_str(), 
                None)?;

        // Ensure that media_status has an entry and take the first
        if let Some(media_status) = media_status.entries.first(){
            let transport_id = app.transport_id.as_str();
            let session_id = media_status.media_session_id;

            // Signal the state to the chromecast
            match state {
                PlayerSignal::Play => {
                    device.media.play(transport_id, session_id)?;
                }
                PlayerSignal::Pause => {
                    device.media.pause(transport_id, session_id)?;
                }
                PlayerSignal::Stop => {
                    device.media.stop(transport_id, session_id)?;
                }
                PlayerSignal::Seek(time) => {
                    device.media.seek(
                        transport_id, session_id,
                        Some(time),     // Time to seek to
                        None)?;         // Resume State (leave state unchanged)
                }
            }
        }else{
            return Err(CastError::CasterError(
                "Cannot change media state. No active media."));
        }
        device.connection.disconnect(DESTINATION_ID).unwrap();
        Ok(())
    }

    /// Create a new CastDevice connection.  
    /// *Note: This connection must either be kept-alive with ping/pong 
    /// or closed after a short period of time.*
    fn connect(&self) -> Result<CastDevice, CastError> {
        let addr = match &self.device_addr {
            Some(addr) => addr.clone(), 
            None => {
                return Err(CastError::CasterError("No device address set."));
            }
        };

        let device = match CastDevice::connect_without_host_verification(
            addr, 
            8009){
                
            Ok(device) => device,
            Err(err) => {
                panic!("Failed to establish connection to device: {:?}", err);
            }
        };
        device.connection.connect(DESTINATION_ID).unwrap();
        Ok(device)
    }
}

/// Uses mDNS discovery to find all available Chromecasts on the local network.
/// ### Returns 
/// `Vec<(String, IpAddr)` - "Friendly name" and IP addresses of chromecasts
pub async fn find_chromecasts() -> Result<Vec<(String, IpAddr)>, CastError> {
    // Create timeout vars
    let timeout = Duration::from_secs(TIMEOUT_SECONDS);
    let start_time = SystemTime::now();
    
    // Create the discovery stream
    let stream = mdns::discover::all(SERVICE_NAME, timeout)?
        .listen()
        .take_while(|_|future::ready(start_time.elapsed().unwrap() < timeout));
    pin_mut!(stream);
    
    // Listen and add devices to vec
    let mut device_ips = Vec::new();
    while let Some(Ok(resp)) = stream.next().await {
        let addr = resp.records()
            .find_map(self::to_ip_addr);
        if let Some(addr) = addr {
            if !device_ips.contains(&addr) {
                device_ips.push(addr.clone());
            }
        }
    }

    // TODO Parallelize name gathering to get all device names available at once

    // Poll the chromecast for their names
    let client = Client::new();
    let mut chromecasts = Vec::<(String, IpAddr)>::new();
    for ip in device_ips {
        // Build the URI to poll the chromecast's description xml
        let uri = format!("http://{}:8008/ssdp/device-desc.xml", ip)
                    .parse()
                    .unwrap();

        // Send a GET request to the chromecast's device XML 
        if let Ok(mut resp) = client.get(uri).await {
            if resp.status().is_success() {
                // Retrieve the response body
                if let Some(body) = resp.body_mut().data().await {
                    // Ensure Hyper didnt error
                    if let Ok(body) = body {
                        // Run the result through regex to pull the name
                        let body = body.to_vec();
                        let body_string = String::from_utf8(body).unwrap();
                        let reg = Regex::new(r#"<friendlyName>(.*)</friendlyName>"#).unwrap();
                        let captures = reg.captures(&body_string);
                        if let Some(captures) = captures {
                            // Push the name into a vec with the IP, if there was a match
                            if let Some(capture) = captures.get(1) {
                                chromecasts.push((capture.as_str().into(), ip));
                                continue;
                            }
                        }
                    }
                }
            }
        }    

        // If for some reason we couldn't get the name, 
        // just call it Unknown and save the ip address
        chromecasts.push((String::from("Unknown"), ip));
    }

    Ok(chromecasts)
}

/// Convert a DNS record to IpAddr
/// ### Returns
/// `Some<IpAddr>` If record is A or AAAA  
/// Otherwise   
/// `None`
fn to_ip_addr(record: &Record) -> Option<IpAddr> {
    match record.kind {
        RecordKind::A(addr) => Some(addr.into()),
        RecordKind::AAAA(addr) => Some(addr.into()),
        _ => None,
    }
}

/// Returns the ip address of the computer running this program.
fn get_local_ip() -> Result<String, std::io::Error> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    Ok(socket.local_addr()?.ip().to_string())
}
