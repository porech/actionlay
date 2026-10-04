use actionlay_media::ffmpeg_info;
use ffmpeg_next as ffmpeg;

#[test]
fn ffmpeg_is_pinned_gpl_and_free() {
    ffmpeg_info::init();
    let info = ffmpeg_info::build_info();
    assert!(info.version.contains("9.0.2"), "version: {}", info.version);
    assert!(info.configuration.contains("--enable-gpl"), "{}", info.configuration);
    assert!(!info.configuration.contains("nonfree"), "{}", info.configuration);
    assert_eq!(info.license, "GPL version 2 or later");
}

#[test]
fn required_decoders_are_present() {
    ffmpeg_info::init();
    for id in [ffmpeg::codec::Id::HEVC, ffmpeg::codec::Id::H264, ffmpeg::codec::Id::AAC] {
        assert!(ffmpeg::decoder::find(id).is_some(), "missing decoder {id:?}");
    }
}
