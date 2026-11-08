//! End-to-end video conversions on clips generated with FFmpeg's own encoders at runtime.
#![cfg(feature = "video")]

use ffmpeg::{codec, encoder, filter, format, frame, media, Dictionary, Packet, Rational};
use ffmpeg_next as ffmpeg;
use morph_core::{
    convert, ConvertOptions, Error, Job, Metadata, OutputFormat, Resize, VideoFormat,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const LOCATION: &str = "+48.8584+002.2945/";

fn graph(sink: &str, spec: &str) -> filter::Graph {
    let mut g = filter::Graph::new();
    g.add(&filter::find(sink).unwrap(), "out", "").unwrap();
    g.input("out", 0).unwrap().parse(spec).unwrap();
    g.validate().unwrap();
    g
}

fn flush(
    enc: &mut encoder::encoder::Encoder,
    octx: &mut format::context::Output,
    idx: usize,
    from: Rational,
) {
    let to = octx.stream(idx).unwrap().time_base();
    let mut p = Packet::empty();
    while enc.receive_packet(&mut p).is_ok() {
        p.set_stream(idx);
        p.rescale_ts(from, to);
        p.write_interleaved(octx).unwrap();
    }
}

/// Writes a 1 s 320x240 15 fps clip with a stereo sine track, in the container deduced from `path`.
fn make_clip(path: &Path, vcodec: &str, acodec: &str, tags: bool) {
    let mut octx = format::output(path).unwrap();
    let global = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

    let mut vg = graph(
        "buffersink",
        "testsrc2=size=320x240:rate=15:duration=1,format=yuv420p",
    );
    let vtb = vg.get("out").unwrap().sink().time_base();
    let vc = encoder::find_by_name(vcodec).unwrap_or_else(|| panic!("{vcodec} missing"));
    let mut vost = octx.add_stream(vc).unwrap();
    let mut venc = codec::context::Context::new_with_codec(vc)
        .encoder()
        .video()
        .unwrap();
    venc.set_width(320);
    venc.set_height(240);
    venc.set_format(format::Pixel::YUV420P);
    venc.set_time_base(vtb);
    venc.set_frame_rate(Some(Rational(15, 1)));
    if global {
        venc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let mut o = Dictionary::new();
    if vcodec == "libvpx-vp9" {
        o.set("deadline", "realtime");
        o.set("cpu-used", "8");
    }
    let mut venc = venc.open_with(o).unwrap();
    vost.set_parameters(&venc);
    let vidx = vost.index();

    let (afmt, sfmt) = if acodec == "aac" {
        ("fltp", format::Sample::F32(format::sample::Type::Planar))
    } else {
        ("s16", format::Sample::I16(format::sample::Type::Packed))
    };
    let mut ag = graph(
        "abuffersink",
        &format!("sine=frequency=440:sample_rate=48000:duration=1,aformat=sample_fmts={afmt}:channel_layouts=stereo"),
    );
    let ac = encoder::find_by_name(acodec).unwrap_or_else(|| panic!("{acodec} missing"));
    let mut aost = octx.add_stream(ac).unwrap();
    let mut aenc = codec::context::Context::new_with_codec(ac)
        .encoder()
        .audio()
        .unwrap();
    aenc.set_rate(48_000);
    aenc.set_channel_layout(ffmpeg::ChannelLayout::STEREO);
    aenc.set_format(sfmt);
    aenc.set_bit_rate(96_000);
    aenc.set_time_base(Rational(1, 48_000));
    if global {
        aenc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let mut aenc = aenc.open_as(ac).unwrap();
    aost.set_parameters(&aenc);
    let aidx = aost.index();
    ag.get("out")
        .unwrap()
        .sink()
        .set_frame_size(aenc.frame_size());

    if tags {
        let mut md = Dictionary::new();
        md.set("title", "Secret holiday");
        md.set("location", LOCATION);
        octx.set_metadata(md);
    }
    octx.write_header().unwrap();

    let mut vf = frame::Video::empty();
    while vg.get("out").unwrap().sink().frame(&mut vf).is_ok() {
        vf.set_kind(ffmpeg::picture::Type::None);
        venc.send_frame(&vf).unwrap();
        flush(&mut venc, &mut octx, vidx, vtb);
    }
    venc.send_eof().unwrap();
    flush(&mut venc, &mut octx, vidx, vtb);

    let mut af = frame::Audio::empty();
    while ag.get("out").unwrap().sink().frame(&mut af).is_ok() {
        aenc.send_frame(&af).unwrap();
        flush(&mut aenc, &mut octx, aidx, Rational(1, 48_000));
    }
    aenc.send_eof().unwrap();
    flush(&mut aenc, &mut octx, aidx, Rational(1, 48_000));
    octx.write_trailer().unwrap();
}

/// Clips in the input containers the app must read.
fn clips(dir: &Path) -> Vec<std::path::PathBuf> {
    // FFmpeg's LGPL build has no H.264/HEVC encoder, so MP4/MOV use MPEG-4 + AAC; the
    // decode path (demuxer, decoder, filters) is the same for phone clips.
    [
        ("a.mp4", "mpeg4", "aac"),
        ("b.mov", "mpeg4", "aac"),
        ("c.mkv", "libvpx-vp9", "libopus"),
        ("d.webm", "libvpx-vp9", "libopus"),
    ]
    .into_iter()
    .map(|(name, v, a)| {
        let p = dir.join(name);
        make_clip(&p, v, a, true);
        p
    })
    .collect()
}

fn job(input: &Path, output: &Path, f: VideoFormat, options: ConvertOptions) -> Job {
    Job {
        input: input.into(),
        output: output.into(),
        format: OutputFormat::Video(f),
        options,
    }
}

fn run(j: &Job) -> morph_core::Result<()> {
    convert(j, &|_| {}, &AtomicBool::new(false))
}

struct Info {
    container: String,
    video: (codec::Id, u32, u32),
    audio: Option<codec::Id>,
    meta: Vec<(String, String)>,
}

fn probe(p: &Path) -> Info {
    let ictx = format::input(p).unwrap();
    let v = ictx.streams().best(media::Type::Video).unwrap();
    let vd = codec::context::Context::from_parameters(v.parameters())
        .unwrap()
        .decoder()
        .video()
        .unwrap();
    Info {
        container: ictx.format().name().to_string(),
        video: (v.parameters().id(), vd.width(), vd.height()),
        audio: ictx
            .streams()
            .best(media::Type::Audio)
            .map(|s| s.parameters().id()),
        meta: ictx
            .metadata()
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.to_string()))
            .collect(),
    }
}

#[test]
fn every_input_container_converts_to_vp9_and_av1() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for input in clips(dir.path()) {
        for (f, id) in [
            (VideoFormat::WebmVp9, codec::Id::VP9),
            (VideoFormat::WebmAv1, codec::Id::AV1),
        ] {
            let out = dir.path().join(format!(
                "{}.{:?}.webm",
                input.file_name().unwrap().to_string_lossy(),
                f
            ));
            run(&job(&input, &out, f, ConvertOptions::default())).unwrap();
            let i = probe(&out);
            assert_eq!(i.video.0, id, "{input:?}");
            assert_eq!((i.video.1, i.video.2), (320, 240));
            assert_eq!(i.audio, Some(codec::Id::OPUS));
            assert!(i.container.contains("webm"));
        }
    }
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".morph-part"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn resize_keeps_aspect_ratio_with_even_sides_and_bitrate_overrides_crf() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mkv");
    make_clip(&input, "libvpx-vp9", "libopus", false);
    let mut opts = ConvertOptions {
        resize: Some(Resize::Fit {
            max_w: 161,
            max_h: 1000,
        }),
        ..Default::default()
    };
    let out = dir.path().join("fit.webm");
    run(&job(&input, &out, VideoFormat::WebmVp9, opts.clone())).unwrap();
    assert_eq!((probe(&out).video.1, probe(&out).video.2), (160, 120));
    opts.resize = Some(Resize::Percent(25));
    opts.video_bitrate_kbps = Some(200);
    let out = dir.path().join("pct.webm");
    run(&job(&input, &out, VideoFormat::WebmVp9, opts)).unwrap();
    assert_eq!((probe(&out).video.1, probe(&out).video.2), (80, 60));
}

#[test]
fn remux_copies_streams_without_reencoding() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_clip(&input, "mpeg4", "aac", false);
    let out = dir.path().join("out.mkv");
    run(&job(
        &input,
        &out,
        VideoFormat::MkvRemux,
        ConvertOptions::default(),
    ))
    .unwrap();
    let i = probe(&out);
    assert!(i.container.contains("matroska"));
    assert_eq!(i.video.0, codec::Id::MPEG4);
    assert_eq!(i.audio, Some(codec::Id::AAC));
}

#[test]
fn metadata_strip_drops_location_and_keep_copies_it() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_clip(&input, "mpeg4", "aac", true);
    assert!(probe(&input).meta.iter().any(|(k, _)| k == "location"));
    for (f, ext) in [
        (VideoFormat::MkvRemux, "mkv"),
        (VideoFormat::WebmVp9, "webm"),
    ] {
        for (m, kept) in [(Metadata::Strip, false), (Metadata::Keep, true)] {
            let out = dir.path().join(format!("{m:?}.{ext}"));
            let o = ConvertOptions {
                metadata: m,
                ..Default::default()
            };
            run(&job(&input, &out, f, o)).unwrap();
            let meta = probe(&out).meta;
            let has = |key: &str| {
                meta.iter()
                    .any(|(k, v)| k == key && (key != "location" || v.contains("48.8584")))
            };
            assert_eq!(has("location"), kept, "{f:?} {m:?}: {meta:?}");
            assert_eq!(has("title"), kept, "{f:?} {m:?}: {meta:?}");
        }
    }
}

#[test]
fn progress_is_monotonic_and_ends_at_one() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.webm");
    make_clip(&input, "libvpx-vp9", "libopus", false);
    let seen = Mutex::new(Vec::new());
    let out = dir.path().join("out.mkv");
    let j = job(
        &input,
        &out,
        VideoFormat::MkvRemux,
        ConvertOptions::default(),
    );
    convert(
        &j,
        &|p| seen.lock().unwrap().push(p),
        &AtomicBool::new(false),
    )
    .unwrap();
    let seen = seen.into_inner().unwrap();
    assert!(seen.len() > 2, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert_eq!(*seen.last().unwrap(), 1.0);
}

#[test]
fn cancel_stops_and_leaves_no_file() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_clip(&input, "mpeg4", "aac", false);
    for f in [VideoFormat::WebmVp9, VideoFormat::MkvRemux] {
        let out = dir.path().join(format!("c.{}", f.extension()));
        // Cancelled from the first progress report, i.e. mid-conversion.
        let cancel = AtomicBool::new(false);
        let r = convert(
            &job(&input, &out, f, ConvertOptions::default()),
            &|_| cancel.store(true, Ordering::Relaxed),
            &cancel,
        );
        assert!(matches!(r, Err(Error::Cancelled)), "{f:?}: {r:?}");
        assert!(!out.exists());
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "only the input remains"
    );
}

#[test]
fn failure_leaves_no_partial_output() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("bad.mp4");
    std::fs::write(&input, b"not a video").unwrap();
    let out = dir.path().join("out.webm");
    assert!(run(&job(
        &input,
        &out,
        VideoFormat::WebmVp9,
        ConvertOptions::default()
    ))
    .is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn gif_is_capped_to_480_wide_without_audio_and_loops() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_clip(&input, "mpeg4", "aac", false);
    let out = dir.path().join("out.gif");
    run(&job(
        &input,
        &out,
        VideoFormat::Gif,
        ConvertOptions::default(),
    ))
    .unwrap();
    let i = probe(&out);
    assert_eq!(i.video.0, codec::Id::GIF);
    assert_eq!((i.video.1, i.video.2), (320, 240)); // already under the cap
    assert_eq!(i.audio, None);
    // NETSCAPE2.0 loop extension, count 0 = forever.
    let bytes = std::fs::read(&out).unwrap();
    let at = bytes
        .windows(11)
        .position(|w| w == b"NETSCAPE2.0")
        .expect("loop extension");
    assert_eq!(&bytes[at + 11..at + 16], &[3, 1, 0, 0, 0]);
    let out = dir.path().join("small.gif");
    let o = ConvertOptions {
        resize: Some(Resize::Percent(50)),
        ..Default::default()
    };
    run(&job(&input, &out, VideoFormat::Gif, o)).unwrap();
    assert_eq!((probe(&out).video.1, probe(&out).video.2), (160, 120));
}

#[test]
fn mp4_h264_uses_the_system_encoder_or_reports_it_missing() {
    ffmpeg::init().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mkv");
    make_clip(&input, "libvpx-vp9", "libopus", false);
    let out = dir.path().join("out.mp4");
    let r = run(&job(
        &input,
        &out,
        VideoFormat::Mp4H264,
        ConvertOptions {
            resize: Some(Resize::Fit {
                max_w: 161,
                max_h: 1000,
            }),
            ..Default::default()
        },
    ));
    if !morph_core::video::h264_available() {
        let e = r.unwrap_err().to_string();
        assert!(e.contains("no system H.264 encoder available"), "{e}");
        assert!(!out.exists());
        eprintln!("no system H.264 encoder: MP4 output test skipped");
        return;
    }
    r.unwrap();
    let i = probe(&out);
    assert_eq!(i.video.0, codec::Id::H264);
    assert_eq!((i.video.1, i.video.2), (160, 120));
    assert_eq!(i.audio, Some(codec::Id::AAC));
    assert!(i.container.contains("mp4"));
    // +faststart: moov precedes mdat.
    let b = std::fs::read(&out).unwrap();
    let pos = |t: &[u8]| b.windows(4).position(|w| w == t).unwrap();
    assert!(pos(b"moov") < pos(b"mdat"));
}
