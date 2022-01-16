use ffmpeg::{
    codec, encoder, format, log, media, Rational,
};

#[allow(dead_code)]
#[derive(Eq, PartialEq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
pub enum Chromecast {
    FirstAndSecond,
    Third,
    Ultra,
    GoogleTV,
    NestHub,
}


/// A list of valid codec pairs (video, audio), Note that these don't work
/// for ALL chromecast generations, but ones not listed here are always 
/// non-compatible. Majority of these are untested and are just based off Google's
/// supported media types list.
#[allow(dead_code)]
const VALID_CODECS: [(codec::Id, codec::Id); 8] = [
    (codec::Id::H264, codec::Id::MP3),
    (codec::Id::H264, codec::Id::AAC),
    (codec::Id::HEVC, codec::Id::MP3),
    (codec::Id::HEVC, codec::Id::AAC),
    (codec::Id::H265, codec::Id::MP3),
    (codec::Id::H265, codec::Id::AAC),
    (codec::Id::VP8, codec::Id::VORBIS),
    (codec::Id::VP9, codec::Id::VORBIS),
];

// TODO convert from &str to Path/PathBuf
// TODO perform error wrapping/handling
/// Test if the video and audio codecs are compatible with specific chromecast
#[allow(dead_code)]
pub fn is_chromecast_compatible(input: &str, _chromecast: Chromecast) -> bool {
    ffmpeg::init().unwrap();

    // TODO check if any of the media streams are compatible, not just best
    let ictx = format::input(&input).unwrap();
    let video_stream = ictx.streams().best(media::Type::Video).unwrap();
    let _audio_stream = ictx.streams().best(media::Type::Audio).unwrap();
    let _vcodec = video_stream.codec();
    
    todo!()
}

/// Extracts the video codec from the best video stream available
/// #### Returns
/// ffmpeg::codec::Id - The id of the video stream codec
#[allow(dead_code)]
pub fn get_video_codec(input: &str) -> ffmpeg::codec::Id {
    ffmpeg::init().unwrap();

    let ictx = format::input(&input).unwrap();
    let ist = ictx.streams().best(media::Type::Video).unwrap();
    ist.codec().id()
}

/// Move media streams from one container to another.
///
/// This is ripped straight from ffmpeg-next examples.
/// https://github.com/zmwangx/rust-ffmpeg/blob/5ed41c84ff877dc9ae9bd76412c86ee03afb5282/examples/remux.rs
/// Bless their soul for providing the multimedia voodoo code.
///
/// #### Usage
/// `remux("media.mkv", "media.mp4");`
#[allow(dead_code)]
pub fn remux(input: &str, output: &str) { 
    //TODO Error handling/wrapping

    ffmpeg::init().unwrap();
    log::set_level(log::Level::Warning);

    let mut ictx = format::input(&input).unwrap();
    let mut octx = format::output(&output).unwrap();

    let mut stream_mapping = vec![0; ictx.nb_streams() as _];
    let mut ist_time_bases = vec![Rational(0, 1); ictx.nb_streams() as _];
    let mut ost_index = 0;
    for (ist_index, ist) in ictx.streams().enumerate() {
        let ist_medium = ist.codec().medium();
        if ist_medium != media::Type::Audio
            && ist_medium != media::Type::Video
            && ist_medium != media::Type::Subtitle
        {
            stream_mapping[ist_index] = -1;
            continue;
        }
        stream_mapping[ist_index] = ost_index;
        ist_time_bases[ist_index] = ist.time_base();
        ost_index += 1;
        let mut ost = octx.add_stream(encoder::find(codec::Id::None)).unwrap();
        ost.set_parameters(ist.parameters());
        // We need to set codec_tag to 0 lest we run into incompatible codec tag
        // issues when muxing into a different container format. Unfortunately
        // there's no high level API to do this (yet).
        unsafe {
            (*ost.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    octx.set_metadata(ictx.metadata().to_owned());
    octx.write_header().unwrap();

    for (stream, mut packet) in ictx.packets() {
        let ist_index = stream.index();
        let ost_index = stream_mapping[ist_index];
        if ost_index < 0 {
            continue;
        }
        let ost = octx.stream(ost_index as _).unwrap();
        packet.rescale_ts(ist_time_bases[ist_index], ost.time_base());
        packet.set_position(-1);
        packet.set_stream(ost_index as _);
        packet.write_interleaved(&mut octx).unwrap();
    }

    octx.write_trailer().unwrap();
}

