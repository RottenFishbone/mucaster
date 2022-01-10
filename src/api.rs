use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Request {
    Cast(CastSignal)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CastSignal {
    Begin(u32),
    Stop,
    Pause,
    Play,
    Seek(f32),
}
