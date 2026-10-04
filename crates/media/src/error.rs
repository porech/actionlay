use ffmpeg_next as ffmpeg;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("FFmpeg: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("the file has no video stream")]
    NoVideoStream,
    #[error("hardware decoding: {0}")]
    Hw(String),
    #[error("audio: {0}")]
    Audio(String),
}
