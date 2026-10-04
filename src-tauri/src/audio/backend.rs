use crate::audio::types::{AppStream, EqConfig, OutputDevice};
use crate::error::SinkError;
use crate::routing_model::OutputBinding;

/// Abstraction over the audio system (the native PipeWire backend, or a
/// recording mock in tests); commands only ever talk to this trait.
pub trait AudioBackend: Send + Sync {
    /// `label` is the human-readable device description shown by system mixers.
    fn create_virtual_sink(&self, name: &str, label: &str) -> Result<(), SinkError>;
    fn destroy_virtual_sink(&self, name: &str) -> Result<(), SinkError>;
    fn list_app_streams(&self) -> Result<Vec<AppStream>, SinkError>;
    fn list_output_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;
    fn set_sink_volume(&self, sink_name: &str, volume_percent: u8) -> Result<(), SinkError>;
    /// Apply a hardware input's Audio FX (gate, compressor, limiter).
    /// Backends without a DSP engine ignore it.
    fn set_input_fx(
        &self,
        _id: &str,
        _fx: &crate::routing_model::FxChain,
    ) -> Result<(), SinkError> {
        Ok(())
    }
    /// Pause or resume level metering. Backends without meters ignore it.
    fn set_meters_active(&self, _active: bool) -> Result<(), SinkError> {
        Ok(())
    }
    fn set_sink_mute(&self, sink_name: &str, muted: bool) -> Result<(), SinkError>;
    /// Move an app stream to a sink. An empty `sink_name` means "unassign":
    /// the stream is returned to the system default sink.
    fn move_stream_to_sink(&self, stream_index: u32, sink_name: &str) -> Result<(), SinkError>;
    fn set_app_volume(&self, stream_index: u32, volume_percent: u8) -> Result<(), SinkError>;

    /// Apply a channel's parametric EQ (insert/re-tune/remove the biquad
    /// chain).
    fn set_channel_eq(&self, sink_name: &str, config: &EqConfig) -> Result<(), SinkError>;

    /// Create a mix bus: a capturable virtual source whose label is the
    /// device name recorders (OBS) display. Native-only.
    fn create_bus(
        &self,
        name: &str,
        label: &str,
        role: crate::persistence::buses::MixRole,
    ) -> Result<(), SinkError>;

    /// Destroy a mix bus (its links go with it).
    fn destroy_bus(&self, name: &str) -> Result<(), SinkError>;

    /// Replace the set of channels feeding a mix bus.
    fn set_bus_members(&self, name: &str, channels: &[String]) -> Result<(), SinkError>;

    /// Set one member's send level within one mix (0-100%; 100 = unity).
    /// Independent of the member's own volume/EQ - only this mix hears it.
    fn set_bus_member_gain(
        &self,
        bus_name: &str,
        member: &str,
        percent: u8,
    ) -> Result<(), SinkError>;

    /// Register a matrix hardware source. Volume/mute apply only to WaveSink's
    /// mix sends, never to the physical device's global PipeWire controls.
    fn set_hardware_input(
        &self,
        id: &str,
        source_name: &str,
        volume_percent: u8,
        muted: bool,
    ) -> Result<(), SinkError>;
    fn remove_hardware_input(&self, id: &str) -> Result<(), SinkError>;

    /// Replace physical playback targets for a mix. Bindings are independent
    /// from session-only monitoring and may contain more than one device.
    fn set_mix_outputs(&self, name: &str, outputs: &[OutputBinding]) -> Result<(), SinkError>;

    /// Monitor a channel/mix/mic on the system default output (session
    /// scoped, an extra passive link set). Native-only.
    fn set_monitor(&self, name: &str, enabled: bool) -> Result<(), SinkError>;

    /// Hardware capture devices (microphones) for the mic chain.
    fn list_input_devices(&self) -> Result<Vec<OutputDevice>, SinkError>;
}
