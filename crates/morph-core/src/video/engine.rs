//! FFmpeg pipelines: transcode to WebM (VP9 / AV1 + Opus), MP4 (system H.264 + AAC) and GIF; remux to Matroska.

use crate::{Error, Job, Metadata, OutputFormat, Resize, Result, VideoFormat};
use ffmpeg::{
    codec, decoder, encoder, ffi, filter, format, frame, media, picture, ChannelLayout, Dictionary,
    Packet, Rational,
};
use ffmpeg_next as ffmpeg;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Once, OnceLock};

/// Why a pipeline stopped.
enum Stop {
    Cancelled,
    Ff(ffmpeg::Error),
    Msg(String),
}

impl From<ffmpeg::Error> for Stop {
    fn from(e: ffmpeg::Error) -> Self {
        Stop::Ff(e)
    }
}

type Run<T> = std::result::Result<T, Stop>;

/// Rate-control family of a video encoder.
#[derive(Clone, Copy, PartialEq)]
enum Rate {
    /// libvpx / libaom: `crf` or bitrate.
    Crf,
    /// System H.264 encoders: quality mode on `h264_mf`, derived bitrate elsewhere.
    H264,
    /// GIF: no rate control.
    None,
}

/// How one `VideoFormat` is produced. Add a row (and an arm in `spec`) for new encoders.
struct Spec {
    container: &'static str,
    video_encoder: &'static str,
    /// Fixed encoder options (speed trade-offs); the rate control is added from the quality.
    video_opts: &'static [(&'static str, &'static str)],
    rate: Rate,
    /// Encoder pixel format, and the filter chain (after the scale) that produces it.
    pix: format::Pixel,
    tail: &'static str,
    /// Fixed output frame rate (GIF).
    fps: Option<u32>,
    /// `None`: the output has no audio.
    audio_encoder: Option<&'static str>,
    /// Container options for `write_header` (`+faststart`, GIF loop).
    mux_opts: &'static [(&'static str, &'static str)],
}

/// GIF frame rate and default width cap.
const GIF_FPS: u32 = 12;
const GIF_MAX_W: u32 = 480;

/// System H.264 encoders in preference order. libx264/libopenh264 are never used (GPL / patents).
const H264_ENCODERS: &[&str] = &["h264_mf", "h264_videotoolbox", "h264_vaapi", "h264_v4l2m2m"];

/// VAAPI works on GPU surfaces: frames are uploaded (`hwupload`) from this render node.
const VAAPI: &str = "h264_vaapi";
const VAAPI_DEVICE: &str = "/dev/dri/renderD128";

/// A new reference to the VAAPI device; the caller `av_buffer_unref`s it.
fn vaapi_device() -> Option<*mut ffi::AVBufferRef> {
    let path = std::ffi::CString::new(VAAPI_DEVICE).ok()?;
    let mut dev = std::ptr::null_mut();
    let r = unsafe {
        ffi::av_hwdevice_ctx_create(
            &mut dev,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            path.as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    (r >= 0).then_some(dev)
}

/// NV12 VAAPI surfaces pool for the probe (the real conversion gets it from `hwupload`).
fn vaapi_frames(dev: *mut ffi::AVBufferRef, w: i32, h: i32) -> Option<*mut ffi::AVBufferRef> {
    unsafe {
        let mut f = ffi::av_hwframe_ctx_alloc(dev);
        if f.is_null() {
            return None;
        }
        let c = (*f).data as *mut ffi::AVHWFramesContext;
        (*c).format = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
        (*c).sw_format = ffi::AVPixelFormat::AV_PIX_FMT_NV12;
        (*c).width = w;
        (*c).height = h;
        (*c).initial_pool_size = 4;
        if ffi::av_hwframe_ctx_init(f) < 0 {
            ffi::av_buffer_unref(&mut f);
            return None;
        }
        Some(f)
    }
}

/// Opens `name` on a small test setup; true when the machine can really encode with it.
fn h264_opens(name: &str) -> bool {
    let Some(codec) = encoder::find_by_name(name) else {
        return false;
    };
    let Ok(mut enc) = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
    else {
        return false;
    };
    enc.set_width(320);
    enc.set_height(240);
    enc.set_format(format::Pixel::NV12);
    enc.set_time_base(Rational(1, 25));
    enc.set_frame_rate(Some(Rational(25, 1)));
    enc.set_bit_rate(500_000);
    let mut dev = std::ptr::null_mut();
    if name == VAAPI {
        let Some(d) = vaapi_device() else {
            return false;
        };
        dev = d;
        let Some(frames) = vaapi_frames(d, 320, 240) else {
            unsafe { ffi::av_buffer_unref(&mut dev) };
            return false;
        };
        enc.set_format(format::Pixel::VAAPI);
        // The codec context owns this reference.
        unsafe { (*enc.as_mut_ptr()).hw_frames_ctx = frames };
    }
    let ok = enc.open_as(codec).is_ok();
    if !dev.is_null() {
        unsafe { ffi::av_buffer_unref(&mut dev) };
    }
    ok
}

/// First system H.264 encoder that really opens on this machine (the probe result is cached).
fn h264_encoder() -> Option<&'static str> {
    static FOUND: OnceLock<Option<&'static str>> = OnceLock::new();
    // Probe on a fresh thread: the GUI thread is a COM STA (drag-and-drop) and
    // Media Foundation (h264_mf) refuses to open there.
    *FOUND.get_or_init(|| {
        std::thread::spawn(|| {
            init();
            H264_ENCODERS.iter().copied().find(|n| h264_opens(n))
        })
        .join()
        .unwrap_or(None)
    })
}

/// True when this machine has a usable system H.264 encoder (the UI hides MP4 otherwise).
pub fn h264_available() -> bool {
    h264_encoder().is_some()
}

fn spec(f: VideoFormat) -> Option<Spec> {
    let webm = |video_encoder, video_opts| Spec {
        container: "webm",
        video_encoder,
        video_opts,
        rate: Rate::Crf,
        pix: format::Pixel::YUV420P,
        tail: "format=yuv420p",
        fps: None,
        audio_encoder: Some("libopus"),
        mux_opts: &[],
    };
    match f {
        VideoFormat::WebmVp9 => Some(webm(
            "libvpx-vp9",
            &[("deadline", "good"), ("cpu-used", "4"), ("row-mt", "1")],
        )),
        VideoFormat::WebmAv1 => Some(webm("libaom-av1", &[("cpu-used", "6"), ("row-mt", "1")])),
        VideoFormat::Mp4H264 => {
            let video_encoder = h264_encoder()?;
            let vaapi = video_encoder == VAAPI;
            Some(Spec {
                container: "mp4",
                video_encoder,
                video_opts: &[],
                rate: Rate::H264,
                pix: if vaapi {
                    format::Pixel::VAAPI
                } else {
                    format::Pixel::NV12
                },
                tail: if vaapi {
                    "format=nv12,hwupload"
                } else {
                    "format=nv12"
                },
                fps: None,
                audio_encoder: Some("aac"),
                mux_opts: &[("movflags", "+faststart")],
            })
        }
        VideoFormat::Gif => Some(Spec {
            container: "gif",
            video_encoder: "gif",
            video_opts: &[],
            rate: Rate::None,
            pix: format::Pixel::PAL8,
            // One graph: palette from the whole clip, then dithered mapping.
            // shortcut: palettegen holds the clip in memory, use per-chunk palettes for long videos.
            tail: "fps=12,split[a][b];[a]palettegen[p];[b][p]paletteuse",
            fps: Some(GIF_FPS),
            audio_encoder: None,
            mux_opts: &[("loop", "0")],
        }),
        _ => None,
    }
}

/// Bitrate (bit/s) for encoders without a quality mode: 0.02..0.2 bit per pixel.
fn auto_bitrate(w: u32, h: u32, fps: Rational, quality: u8) -> usize {
    let fps = if fps.denominator() > 0 && fps.numerator() > 0 {
        f64::from(fps.numerator()) / f64::from(fps.denominator())
    } else {
        30.0
    };
    let bpp = 0.02 + 0.18 * f64::from(quality.clamp(1, 100)) / 100.0;
    (f64::from(w) * f64::from(h) * fps * bpp) as usize
}

/// Quality 1..=100 to CRF: 100 -> 15, 1 -> 50 (valid for libvpx-vp9 and libaom).
fn crf(quality: u8) -> u32 {
    let q = u32::from(quality.clamp(1, 100)) - 1;
    50 - (q * 35 + 49) / 99
}

/// Output size: resize applied, aspect ratio kept, never upscaled by `Fit`, both sides even.
fn target_dims(w: u32, h: u32, resize: Option<Resize>) -> (u32, u32) {
    let s = match resize {
        None => 1.0,
        Some(Resize::Percent(p)) => f64::from(p.max(1)) / 100.0,
        Some(Resize::Fit { max_w, max_h }) => {
            let lim = |max: u32, v: u32| {
                if max == 0 {
                    f64::INFINITY
                } else {
                    f64::from(max) / f64::from(v)
                }
            };
            lim(max_w, w).min(lim(max_h, h)).min(1.0)
        }
        Some(Resize::ShortSide(s)) => (f64::from(s) / f64::from(w.min(h))).min(1.0),
    };
    let even = |v: u32| ((f64::from(v) * s / 2.0).floor() as u32).max(1) * 2;
    (even(w), even(h))
}

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        let _ = ffmpeg::init();
        ffmpeg::log::set_level(ffmpeg::log::Level::Error);
    });
}

pub fn convert(job: &Job, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Result<()> {
    let OutputFormat::Video(fmt) = job.format else {
        return Err(Error::Unsupported("not a video format".into()));
    };
    if fmt == VideoFormat::Mp4H264 && !h264_available() {
        return Err(Error::Unsupported(
            "no system H.264 encoder available".into(),
        ));
    }
    let (container, spec) = match fmt {
        VideoFormat::MkvRemux => ("matroska", None),
        f => match spec(f) {
            Some(s) => (s.container, Some(s)),
            None => return Err(Error::Unsupported(format!("video output {f:?}"))),
        },
    };
    init();

    // Written next to the target and renamed on success, so no partial output survives.
    let mut tmp = job.output.clone().into_os_string();
    tmp.push(".morph-part");
    let tmp = PathBuf::from(tmp);
    let _cleanup = Cleanup(&tmp);

    let done = run(job, spec.as_ref(), container, &tmp, progress, cancel);
    match done {
        Ok(()) => {
            std::fs::rename(&tmp, &job.output)?;
            progress(1.0);
            Ok(())
        }
        Err(Stop::Cancelled) => Err(Error::Cancelled),
        Err(Stop::Ff(e)) => Err(Error::Unsupported(format!("ffmpeg: {e}"))),
        Err(Stop::Msg(m)) => Err(Error::Unsupported(m)),
    }
}

/// Removes the temp file on drop (a no-op once it has been renamed).
struct Cleanup<'a>(&'a Path);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

fn run(
    job: &Job,
    spec: Option<&Spec>,
    container: &str,
    tmp: &Path,
    progress: &dyn Fn(f32),
    cancel: &AtomicBool,
) -> Run<()> {
    let mut ictx = format::input(&job.input)?;
    let mut octx = format::output_as(tmp, container)?;
    if job.options.metadata == Metadata::Keep {
        octx.set_metadata(ictx.metadata().to_owned());
    }
    let total = ictx.duration();
    let report = |pts: Option<i64>, tb: Rational| {
        if let (Some(pts), true) = (pts, total > 0) {
            let secs = pts as f64 * f64::from(tb.numerator()) / f64::from(tb.denominator().max(1));
            progress((secs * 1e6 / total as f64).clamp(0.0, 0.99) as f32);
        }
    };

    let vidx = ictx
        .streams()
        .best(media::Type::Video)
        .map(|s| s.index())
        .ok_or_else(|| Stop::Msg("no video stream".into()))?;
    let aidx = ictx
        .streams()
        .find(|s| s.parameters().medium() == media::Type::Audio)
        .map(|s| s.index());

    match spec {
        None => remux(&mut ictx, &mut octx, vidx, aidx, cancel, &report),
        Some(spec) => transcode(job, spec, &mut ictx, &mut octx, vidx, aidx, cancel, &report),
    }
}

fn remux(
    ictx: &mut format::context::Input,
    octx: &mut format::context::Output,
    vidx: usize,
    aidx: Option<usize>,
    cancel: &AtomicBool,
    report: &dyn Fn(Option<i64>, Rational),
) -> Run<()> {
    let mut map = vec![None; ictx.nb_streams() as usize];
    let mut in_tb = vec![Rational(0, 1); map.len()];
    for (n, i) in [Some(vidx), aidx].into_iter().flatten().enumerate() {
        let ist = ictx.stream(i).ok_or(ffmpeg::Error::StreamNotFound)?;
        let mut ost = octx.add_stream(encoder::find(codec::Id::None))?;
        ost.set_parameters(ist.parameters());
        // A tag from the source container may be invalid in Matroska.
        unsafe { (*ost.parameters().as_mut_ptr()).codec_tag = 0 };
        map[i] = Some(n);
        in_tb[i] = ist.time_base();
    }
    octx.write_header()?;
    for (stream, mut packet) in ictx.packets() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let i = stream.index();
        let Some(o) = map[i] else { continue };
        if i == vidx {
            report(packet.pts().or(packet.dts()), in_tb[i]);
        }
        packet.rescale_ts(
            in_tb[i],
            octx.stream(o)
                .ok_or(ffmpeg::Error::StreamNotFound)?
                .time_base(),
        );
        packet.set_position(-1);
        packet.set_stream(o);
        packet.write_interleaved(octx)?;
    }
    octx.write_trailer()?;
    Ok(())
}

/// Decoder -> filter graph -> encoder for one stream.
struct Lane<D, E> {
    in_idx: usize,
    out_idx: usize,
    in_tb: Rational,
    dec: D,
    graph: filter::Graph,
    enc: E,
    out_tb: Rational,
}

fn global_header(octx: &format::context::Output) -> bool {
    octx.format().flags().contains(format::Flags::GLOBAL_HEADER)
}

#[allow(clippy::too_many_arguments)]
fn transcode(
    job: &Job,
    spec: &Spec,
    ictx: &mut format::context::Input,
    octx: &mut format::context::Output,
    vidx: usize,
    aidx: Option<usize>,
    cancel: &AtomicBool,
    report: &dyn Fn(Option<i64>, Rational),
) -> Run<()> {
    let opts = &job.options;
    let mut v = video_lane(ictx, octx, vidx, spec, opts)?;
    let aidx = aidx.filter(|_| spec.audio_encoder.is_some());
    let mut a = aidx.map(|i| audio_lane(ictx, octx, i, spec)).transpose()?;
    let mut mux = Dictionary::new();
    for (k, v) in spec.mux_opts {
        mux.set(k, v);
    }
    octx.write_header_with(mux)?;
    v.out_tb = octx
        .stream(v.out_idx)
        .ok_or(ffmpeg::Error::StreamNotFound)?
        .time_base();
    if let Some(a) = a.as_mut() {
        a.out_tb = octx
            .stream(a.out_idx)
            .ok_or(ffmpeg::Error::StreamNotFound)?
            .time_base();
    }

    for (stream, packet) in ictx.packets() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let i = stream.index();
        if i == v.in_idx {
            report(packet.pts().or(packet.dts()), v.in_tb);
            v.dec.send_packet(&packet)?;
            pump_video(&mut v, octx, cancel)?;
        } else if let Some(a) = a.as_mut().filter(|a| a.in_idx == i) {
            a.dec.send_packet(&packet)?;
            pump_audio(a, octx, cancel)?;
        }
    }

    v.dec.send_eof()?;
    pump_video(&mut v, octx, cancel)?;
    v.graph
        .get("in")
        .ok_or(ffmpeg::Error::FilterNotFound)?
        .source()
        .flush()?;
    drain_video(&mut v, octx)?;
    v.enc.send_eof()?;
    {
        let (i, f, t) = (v.out_idx, v.enc_tb(), v.out_tb);
        write_packets(|p| v.enc.receive_packet(p).is_ok(), i, octx, f, t)?;
    }
    if let Some(a) = a.as_mut() {
        a.dec.send_eof()?;
        pump_audio(a, octx, cancel)?;
        a.graph
            .get("in")
            .ok_or(ffmpeg::Error::FilterNotFound)?
            .source()
            .flush()?;
        drain_audio(a, octx)?;
        a.enc.send_eof()?;
        {
            let (i, f, t) = (a.out_idx, a.enc_tb(), a.out_tb);
            write_packets(|p| a.enc.receive_packet(p).is_ok(), i, octx, f, t)?;
        }
    }
    octx.write_trailer()?;
    Ok(())
}

type VLane = Lane<decoder::Video, encoder::video::Encoder>;
type ALane = Lane<decoder::Audio, encoder::audio::Encoder>;

impl VLane {
    fn enc_tb(&self) -> Rational {
        self.enc.time_base()
    }
}
impl ALane {
    fn enc_tb(&self) -> Rational {
        self.enc.time_base()
    }
}

fn video_lane(
    ictx: &format::context::Input,
    octx: &mut format::context::Output,
    idx: usize,
    spec: &Spec,
    opts: &crate::ConvertOptions,
) -> Run<VLane> {
    let ist = ictx.stream(idx).ok_or(ffmpeg::Error::StreamNotFound)?;
    let in_tb = ist.time_base();
    let dec = codec::context::Context::from_parameters(ist.parameters())?
        .decoder()
        .video()?;
    // GIFs are capped to 480 px wide unless the user chose a size.
    let resize = opts.resize.or(spec.fps.map(|_| Resize::Fit {
        max_w: GIF_MAX_W,
        max_h: 0,
    }));
    let (w, h) = target_dims(dec.width(), dec.height(), resize);
    let sar = dec.aspect_ratio();
    let sar = if sar.numerator() > 0 && sar.denominator() > 0 {
        sar
    } else {
        Rational(1, 1)
    };

    let mut graph = filter::Graph::new();
    let args = format!(
        "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect={}/{}",
        dec.width(),
        dec.height(),
        ffi::AVPixelFormat::from(dec.format()) as i32,
        in_tb.numerator(),
        in_tb.denominator(),
        sar.numerator(),
        sar.denominator()
    );
    graph.add(
        &filter::find("buffer").ok_or(ffmpeg::Error::FilterNotFound)?,
        "in",
        &args,
    )?;
    graph.add(
        &filter::find("buffersink").ok_or(ffmpeg::Error::FilterNotFound)?,
        "out",
        "",
    )?;
    graph
        .output("in", 0)?
        .input("out", 0)?
        .parse(&format!("scale={w}:{h}:flags=bicubic,{}", spec.tail))?;
    if spec.video_encoder == VAAPI {
        attach_vaapi_device(&mut graph)?;
    }
    graph.validate()?;
    let sink_tb = graph
        .get("out")
        .ok_or(ffmpeg::Error::FilterNotFound)?
        .sink()
        .time_base();

    let codec = encoder::find_by_name(spec.video_encoder).ok_or_else(|| {
        Stop::Msg(format!(
            "encoder {} missing from this FFmpeg build",
            spec.video_encoder
        ))
    })?;
    let global = global_header(octx);
    let mut ost = octx.add_stream(codec)?;
    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    enc.set_width(w);
    enc.set_height(h);
    enc.set_format(spec.pix);
    enc.set_aspect_ratio(sar);
    enc.set_time_base(sink_tb);
    let fr = spec
        .fps
        .map_or_else(|| ist.avg_frame_rate(), |f| Rational(f as i32, 1));
    if fr.numerator() > 0 && fr.denominator() > 0 {
        enc.set_frame_rate(Some(fr));
    }
    if global {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    if spec.video_encoder == VAAPI {
        // Surfaces come from the graph's `hwupload`; the codec context takes its own reference.
        unsafe {
            let sink = graph.get("out").ok_or(ffmpeg::Error::FilterNotFound)?;
            let frames = ffi::av_buffersink_get_hw_frames_ctx(sink.as_ptr() as *mut _);
            if frames.is_null() {
                return Err(Stop::Msg(
                    "VAAPI: no hardware frames from the filter graph".into(),
                ));
            }
            (*enc.as_mut_ptr()).hw_frames_ctx = ffi::av_buffer_ref(frames);
        }
    }
    let mut o = Dictionary::new();
    for (k, v) in spec.video_opts {
        o.set(k, v);
    }
    match (spec.rate, opts.video_bitrate_kbps) {
        (Rate::None, _) => {}
        (_, Some(kbps)) => enc.set_bit_rate(kbps as usize * 1000),
        (Rate::Crf, None) => {
            // Constant-quality mode: CRF with an unconstrained bitrate.
            enc.set_bit_rate(0);
            o.set("crf", &crf(opts.quality).to_string());
        }
        (Rate::H264, None) if spec.video_encoder == "h264_mf" => {
            o.set("rate_control", "quality");
            o.set("quality", &opts.quality.clamp(1, 100).to_string());
        }
        (Rate::H264, None) => enc.set_bit_rate(auto_bitrate(w, h, fr, opts.quality)),
    }
    let enc = enc.open_with(o)?;
    ost.set_parameters(&enc);
    Ok(Lane {
        in_idx: idx,
        out_idx: ost.index(),
        in_tb,
        dec,
        graph,
        enc,
        out_tb: Rational(1, 1),
    })
}

/// Gives every `hwupload` filter of the graph the VAAPI device (parse() creates them unnamed).
fn attach_vaapi_device(graph: &mut filter::Graph) -> Run<()> {
    let mut dev = vaapi_device().ok_or_else(|| Stop::Msg("VAAPI device unavailable".into()))?;
    unsafe {
        let g = graph.as_mut_ptr();
        for i in 0..(*g).nb_filters as usize {
            let f = *(*g).filters.add(i);
            let name = std::ffi::CStr::from_ptr((*(*f).filter).name);
            if name.to_bytes() == b"hwupload" {
                (*f).hw_device_ctx = ffi::av_buffer_ref(dev);
            }
        }
        ffi::av_buffer_unref(&mut dev);
    }
    Ok(())
}

fn audio_lane(
    ictx: &format::context::Input,
    octx: &mut format::context::Output,
    idx: usize,
    spec: &Spec,
) -> Run<ALane> {
    let ist = ictx.stream(idx).ok_or(ffmpeg::Error::StreamNotFound)?;
    let in_tb = ist.time_base();
    let dec = codec::context::Context::from_parameters(ist.parameters())?
        .decoder()
        .audio()?;
    let in_layout = if dec.channel_layout().bits() != 0 {
        dec.channel_layout()
    } else {
        ChannelLayout::default(i32::from(dec.channels()))
    };
    // Mono stays mono, everything else is mixed down to stereo. Native AAC wants planar float.
    let aac = spec.audio_encoder == Some("aac");
    let (layout, lname, kbps) = match (in_layout.channels() == 1, aac) {
        (true, false) => (ChannelLayout::MONO, "mono", 64_000),
        (true, true) => (ChannelLayout::MONO, "mono", 96_000),
        (false, false) => (ChannelLayout::STEREO, "stereo", 96_000),
        (false, true) => (ChannelLayout::STEREO, "stereo", 128_000),
    };
    let (sfmt, sample) = if aac {
        ("fltp", format::Sample::F32(format::sample::Type::Planar))
    } else {
        ("s16", format::Sample::I16(format::sample::Type::Packed))
    };

    let mut graph = filter::Graph::new();
    let args = format!(
        "time_base={}/{}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        in_tb.numerator(),
        in_tb.denominator(),
        dec.rate(),
        dec.format().name(),
        in_layout.bits()
    );
    graph.add(
        &filter::find("abuffer").ok_or(ffmpeg::Error::FilterNotFound)?,
        "in",
        &args,
    )?;
    graph.add(
        &filter::find("abuffersink").ok_or(ffmpeg::Error::FilterNotFound)?,
        "out",
        "",
    )?;
    graph.output("in", 0)?.input("out", 0)?.parse(&format!(
        "aformat=sample_fmts={sfmt}:sample_rates=48000:channel_layouts={lname}"
    ))?;
    graph.validate()?;

    let name = spec.audio_encoder.ok_or(ffmpeg::Error::EncoderNotFound)?;
    let codec = encoder::find_by_name(name)
        .ok_or_else(|| Stop::Msg(format!("encoder {name} missing from this FFmpeg build")))?;
    let global = global_header(octx);
    let mut ost = octx.add_stream(codec)?;
    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()?;
    enc.set_rate(48_000);
    enc.set_channel_layout(layout);
    enc.set_format(sample);
    enc.set_bit_rate(kbps);
    enc.set_time_base(Rational(1, 48_000));
    if global {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let enc = enc.open_as(codec)?;
    ost.set_parameters(&enc);
    graph
        .get("out")
        .ok_or(ffmpeg::Error::FilterNotFound)?
        .sink()
        .set_frame_size(enc.frame_size());
    Ok(Lane {
        in_idx: idx,
        out_idx: ost.index(),
        in_tb,
        dec,
        graph,
        enc,
        out_tb: Rational(1, 1),
    })
}

fn write_packets(
    mut recv: impl FnMut(&mut Packet) -> bool,
    out_idx: usize,
    octx: &mut format::context::Output,
    from: Rational,
    to: Rational,
) -> Run<()> {
    let mut pkt = Packet::empty();
    while recv(&mut pkt) {
        pkt.set_stream(out_idx);
        pkt.rescale_ts(from, to);
        pkt.write_interleaved(octx)?;
    }
    Ok(())
}

fn pump_video(v: &mut VLane, octx: &mut format::context::Output, cancel: &AtomicBool) -> Run<()> {
    let mut f = frame::Video::empty();
    while v.dec.receive_frame(&mut f).is_ok() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let ts = f.timestamp();
        f.set_pts(ts);
        v.graph
            .get("in")
            .ok_or(ffmpeg::Error::FilterNotFound)?
            .source()
            .add(&f)?;
        drain_video(v, octx)?;
    }
    Ok(())
}

fn drain_video(v: &mut VLane, octx: &mut format::context::Output) -> Run<()> {
    let mut out = frame::Video::empty();
    while v
        .graph
        .get("out")
        .ok_or(ffmpeg::Error::FilterNotFound)?
        .sink()
        .frame(&mut out)
        .is_ok()
    {
        out.set_kind(picture::Type::None);
        v.enc.send_frame(&out)?;
        {
            let (i, f, t) = (v.out_idx, v.enc_tb(), v.out_tb);
            write_packets(|p| v.enc.receive_packet(p).is_ok(), i, octx, f, t)?;
        }
    }
    Ok(())
}

fn pump_audio(a: &mut ALane, octx: &mut format::context::Output, cancel: &AtomicBool) -> Run<()> {
    let mut f = frame::Audio::empty();
    while a.dec.receive_frame(&mut f).is_ok() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Stop::Cancelled);
        }
        let ts = f.timestamp();
        f.set_pts(ts);
        a.graph
            .get("in")
            .ok_or(ffmpeg::Error::FilterNotFound)?
            .source()
            .add(&f)?;
        drain_audio(a, octx)?;
    }
    Ok(())
}

fn drain_audio(a: &mut ALane, octx: &mut format::context::Output) -> Run<()> {
    let mut out = frame::Audio::empty();
    while a
        .graph
        .get("out")
        .ok_or(ffmpeg::Error::FilterNotFound)?
        .sink()
        .frame(&mut out)
        .is_ok()
    {
        a.enc.send_frame(&out)?;
        {
            let (i, f, t) = (a.out_idx, a.enc_tb(), a.out_tb);
            write_packets(|p| a.enc.receive_packet(p).is_ok(), i, octx, f, t)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crf_follows_quality() {
        assert_eq!(crf(100), 15);
        assert_eq!(crf(1), 50);
        assert!(crf(90) < crf(50));
    }

    #[test]
    fn dims_are_even_and_keep_aspect() {
        assert_eq!(target_dims(321, 241, None), (320, 240));
        assert_eq!(
            target_dims(1920, 1080, Some(Resize::Percent(50))),
            (960, 540)
        );
        assert_eq!(
            target_dims(
                1920,
                1080,
                Some(Resize::Fit {
                    max_w: 640,
                    max_h: 640
                })
            ),
            (640, 360)
        );
        assert_eq!(
            target_dims(
                320,
                240,
                Some(Resize::Fit {
                    max_w: 1000,
                    max_h: 1000
                })
            ),
            (320, 240)
        );
        // 720p: landscape and portrait, never upscaled.
        let p720 = Some(Resize::ShortSide(720));
        assert_eq!(target_dims(1920, 1080, p720), (1280, 720));
        assert_eq!(target_dims(1080, 1920, p720), (720, 1280));
        assert_eq!(target_dims(640, 360, p720), (640, 360));
    }
}
