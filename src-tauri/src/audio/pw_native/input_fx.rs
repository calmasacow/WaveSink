//! Per-input Audio FX: captures a hardware input (a mic, a capture card),
//! runs the gate → compressor → limiter chain, and plays the processed signal
//! back through a stream the loop links into the input's mixes in place of
//! the raw device. Only built while at least one stage is on.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use pipewire as pw;
use pw::spa;
use spa::pod::Pod;

use crate::audio::pw_native::dsp::{DspChain, DspSettings};
use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::ring::Ring;
use crate::error::SinkError;
use crate::routing_model::FxChain;

/// node.name prefixes of the FX helper streams (under INTERNAL_PREFIX, so
/// they never show up in app/stream listings).
pub const FX_CAPTURE_PREFIX: &str = "sink-internal-fx-capture-";
pub const FX_PLAYBACK_PREFIX: &str = "sink-internal-fx-playback-";
/// Level-store key prefix for an input's post-FX meter.
pub const FX_LEVEL_PREFIX: &str = "fx:";

/// Live-tunable stage settings, shared with the RT capture callback.
pub struct FxParams {
    gate: AtomicBool,
    comp: AtomicBool,
    limiter: AtomicBool,
    gate_threshold_bits: AtomicU32,
    comp_threshold_bits: AtomicU32,
    comp_ratio_bits: AtomicU32,
    limiter_ceiling_bits: AtomicU32,
}

impl FxParams {
    fn new(fx: &FxChain) -> Self {
        let p = Self {
            gate: AtomicBool::new(false),
            comp: AtomicBool::new(false),
            limiter: AtomicBool::new(false),
            gate_threshold_bits: AtomicU32::new(0),
            comp_threshold_bits: AtomicU32::new(0),
            comp_ratio_bits: AtomicU32::new(0),
            limiter_ceiling_bits: AtomicU32::new(0),
        };
        p.apply(fx);
        p
    }

    /// Re-tune in place - no stream rebuild, so a change never clicks.
    pub fn apply(&self, fx: &FxChain) {
        let fx = fx.clamped();
        self.gate.store(fx.gate_enabled, Ordering::Relaxed);
        self.comp.store(fx.compressor_enabled, Ordering::Relaxed);
        self.limiter.store(fx.limiter_enabled, Ordering::Relaxed);
        self.gate_threshold_bits
            .store(fx.gate_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp_threshold_bits
            .store(fx.compressor_threshold_db.to_bits(), Ordering::Relaxed);
        self.comp_ratio_bits
            .store(fx.compressor_ratio.to_bits(), Ordering::Relaxed);
        self.limiter_ceiling_bits
            .store(fx.limiter_ceiling_db.to_bits(), Ordering::Relaxed);
    }

    fn settings(&self) -> DspSettings {
        DspSettings {
            gate_enabled: self.gate.load(Ordering::Relaxed),
            comp_enabled: self.comp.load(Ordering::Relaxed),
            limiter_enabled: self.limiter.load(Ordering::Relaxed),
            // Level and mute live downstream (the route gains), not here.
            gain: 1.0,
            muted: false,
            gate_threshold_db: f32::from_bits(self.gate_threshold_bits.load(Ordering::Relaxed)),
            comp_threshold_db: f32::from_bits(self.comp_threshold_bits.load(Ordering::Relaxed)),
            comp_ratio: f32::from_bits(self.comp_ratio_bits.load(Ordering::Relaxed)),
            limiter_ceiling_db: f32::from_bits(self.limiter_ceiling_bits.load(Ordering::Relaxed)),
        }
    }
}

struct FxCaptureCtx {
    chain: DspChain,
    params: Arc<FxParams>,
    ring: Arc<Ring>,
    levels: Arc<LevelStore>,
    level_slot: usize,
    scratch: Vec<f32>,
}

struct FxPlaybackCtx {
    ring: Arc<Ring>,
}

pub struct InputFxHandle {
    _capture: pw::stream::StreamRc,
    _capture_listener: pw::stream::StreamListener<FxCaptureCtx>,
    playback: pw::stream::StreamRc,
    _playback_listener: pw::stream::StreamListener<FxPlaybackCtx>,
    pub params: Arc<FxParams>,
    /// The device this chain captures; a different device means a rebuild.
    pub source_name: String,
}

impl InputFxHandle {
    /// Node id of the playback stream. Only valid once the server has created
    /// the stream's node - callers must filter the u32::MAX sentinel.
    pub fn playback_node_id(&self) -> u32 {
        self.playback.node_id()
    }
}

/// Stereo F32 format pod for stream negotiation.
fn stereo_f32_format() -> Result<Vec<u8>, SinkError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_channels(2);
    let object = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(c, _)| c.into_inner())
    .map_err(|e| SinkError::Config(format!("fx format pod: {e:?}")))
}

/// The DSP runs on interleaved stereo as one stream at twice the frame rate:
/// time constants stay right and both channels share one detector (linked
/// stereo), so the image never shifts when the gate or compressor acts.
fn interleaved_rate(frame_rate: f32) -> f32 {
    frame_rate * 2.0
}

impl InputFxHandle {
    /// Build both streams against a live hardware source node.
    pub fn new(
        core: &pw::core::CoreRc,
        input_id: &str,
        source_name: &str,
        source_id: u32,
        fx: &FxChain,
        levels: Arc<LevelStore>,
    ) -> Result<Self, SinkError> {
        let err = |stage: &str, e: pw::Error| SinkError::Config(format!("fx {stage}: {e}"));
        let params = Arc::new(FxParams::new(fx));
        let level_slot = levels
            .slot_for(&format!("{FX_LEVEL_PREFIX}{input_id}"))
            .ok_or_else(|| SinkError::Config(format!("meter budget exhausted for {input_id}")))?;
        // Interleaved stereo: ~85 ms of headroom at 48 kHz; real added
        // latency is one quantum.
        let ring = Arc::new(Ring::new(8192));
        // Node names must be plain: the input id carries "hardware:<node>".
        let key: String = input_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        // Capture stage, NOT passive: it must hold the device running, since
        // the processed stream is the only thing feeding the mixes.
        let capture_name = format!("{FX_CAPTURE_PREFIX}{key}");
        let capture = pw::stream::StreamRc::new(
            core.clone(),
            &capture_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Capture",
                "node.name" => capture_name.as_str(),
                "target.object" => source_name,
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("capture stream", e))?;

        let capture_listener = capture
            .add_local_listener_with_user_data(FxCaptureCtx {
                chain: DspChain::new(interleaved_rate(48000.0)),
                params: params.clone(),
                ring: ring.clone(),
                levels,
                level_slot,
                scratch: Vec::with_capacity(8192),
            })
            .param_changed(|_, ctx, id, param| {
                // Track the negotiated rate so DSP time constants are right.
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Some(param) = param else { return };
                let mut info = spa::param::audio::AudioInfoRaw::new();
                if info.parse(param).is_ok() && info.rate() > 0 {
                    ctx.chain = DspChain::new(interleaved_rate(info.rate() as f32));
                }
            })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let valid = data.chunk().size() as usize;
                let Some(bytes) = data.data() else { return };

                let n = (valid.min(bytes.len())) / 4;
                ctx.scratch.clear();
                ctx.scratch.extend(
                    bytes[..n * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_ne_bytes(*b)),
                );

                let settings = ctx.params.settings();
                ctx.chain.process(&mut ctx.scratch, &settings);

                // Post-FX level for the input's meter.
                let mut peaks = [0.0f32; 2];
                for (i, s) in ctx.scratch.iter().enumerate() {
                    let ch = i & 1;
                    peaks[ch] = peaks[ch].max(s.abs());
                }
                ctx.levels.raise(ctx.level_slot, 0, peaks[0]);
                ctx.levels.raise(ctx.level_slot, 1, peaks[1]);

                ctx.ring.push(&ctx.scratch);
            })
            .register()
            .map_err(|e| err("capture listener", e))?;

        let format = stereo_f32_format()?;
        let mut capture_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("fx capture format pod invalid".into()))?];
        capture
            .connect(
                spa::utils::Direction::Input,
                Some(source_id),
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut capture_params,
            )
            .map_err(|e| err("capture connect", e))?;

        // Playback stage: node.autoconnect=false keeps WirePlumber from routing
        // this to the default sink; the link police in thread.rs backs it up.
        let playback_name = format!("{FX_PLAYBACK_PREFIX}{key}");
        let playback = pw::stream::StreamRc::new(
            core.clone(),
            &playback_name,
            pw::properties::properties! {
                "media.type" => "Audio",
                "media.category" => "Playback",
                "node.name" => playback_name.as_str(),
                "node.autoconnect" => "false",
                "node.dont-reconnect" => "true",
            },
        )
        .map_err(|e| err("playback stream", e))?;

        let playback_listener = playback
            .add_local_listener_with_user_data(FxPlaybackCtx { ring })
            .process(|stream, ctx| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Fill only what the graph asked for this cycle (frames);
                // interleaved stereo = 2 samples, 8 bytes per frame.
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                let Some(data) = datas.first_mut() else {
                    return;
                };
                let max_bytes = data.data().map(|d| d.len()).unwrap_or(0);
                let max_frames = max_bytes / 8;
                let frames = if requested > 0 {
                    requested.min(max_frames)
                } else {
                    max_frames.min(1024)
                };
                if frames == 0 {
                    return;
                }
                if let Some(bytes) = data.data() {
                    let mut chunk_samples = [0.0f32; 1024];
                    let total_samples = frames * 2;
                    let mut written = 0;
                    while written < total_samples {
                        let take = (total_samples - written).min(chunk_samples.len());
                        ctx.ring.pop(&mut chunk_samples[..take]);
                        for (i, s) in chunk_samples[..take].iter().enumerate() {
                            let off = (written + i) * 4;
                            bytes[off..off + 4].copy_from_slice(&s.to_ne_bytes());
                        }
                        written += take;
                    }
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 8;
                *chunk.size_mut() = (frames * 8) as u32;
            })
            .register()
            .map_err(|e| err("playback listener", e))?;

        let mut playback_params = [Pod::from_bytes(&format)
            .ok_or_else(|| SinkError::Config("fx playback format pod invalid".into()))?];
        playback
            .connect(
                spa::utils::Direction::Output,
                None,
                // No AUTOCONNECT: the loop creates the links itself.
                pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
                &mut playback_params,
            )
            .map_err(|e| err("playback connect", e))?;

        Ok(Self {
            _capture: capture,
            _capture_listener: capture_listener,
            playback,
            _playback_listener: playback_listener,
            params,
            source_name: source_name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_stereo_runs_at_twice_the_frame_rate() {
        assert_eq!(interleaved_rate(48000.0), 96000.0);
    }

    #[test]
    fn params_take_clamped_values() {
        let fx = FxChain {
            gate_enabled: true,
            gate_threshold_db: f32::NAN,
            compressor_ratio: 500.0,
            ..FxChain::default()
        };
        let p = FxParams::new(&fx).settings();
        assert!(p.gate_enabled);
        assert!(!p.comp_enabled);
        assert_eq!(p.gate_threshold_db, -40.0);
        assert_eq!(p.comp_ratio, 20.0);
        assert_eq!(p.gain, 1.0);
    }
}
