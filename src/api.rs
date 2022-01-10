#[derive(Debug, Clone, Copy)]
pub enum Request {
    Cast(CastSignal)
}

#[derive(Debug, Clone, Copy)]
pub enum CastSignal {
    Begin(u32),
    Stop,
    Pause,
    Play,
    Seek(u32),
}
