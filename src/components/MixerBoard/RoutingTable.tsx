import { useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_VOLUME, type RouteCell } from "../../types";
import { Ms, channelIcon } from "../Icons";
import { Modal } from "../Modal";
import { CHANNEL_COLORS, CHANNEL_ICON_IDS, ChannelIcon } from "../ChannelIcon";
import { EqModal } from "../Eq/EqModal";

const blankCell: RouteCell = { enabled: false, send_percent: 100, muted: false };

export function RoutingTable() {
  const routing = useMixerStore((s) => s.routing);
  const channels = useMixerStore((s) => s.channels);
  const buses = useMixerStore((s) => s.buses);
  const outputs = useMixerStore((s) => s.outputDevices);
  const levels = useMixerStore((s) => s.levels);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const micConfig = useMixerStore((s) => s.micConfig);
  const setRouteCell = useMixerStore((s) => s.setRouteCell);
  const setChannelVolume = useMixerStore((s) => s.setChannelVolume);
  const setMicConfig = useMixerStore((s) => s.setMicConfig);
  const toggleMute = useMixerStore((s) => s.toggleMute);
  const setMixOutputs = useMixerStore((s) => s.setMixOutputs);
  const setBusMute = useMixerStore((s) => s.setBusMute);
  const renameBus = useMixerStore((s) => s.renameBus);
  const setBusIcon = useMixerStore((s) => s.setBusIcon);
  const removeBus = useMixerStore((s) => s.removeBus);
  const addChannel = useMixerStore((s) => s.addChannel);
  const addBus = useMixerStore((s) => s.addBus);
  const addHardwareInput = useMixerStore((s) => s.addHardwareInput);
  const setInputLevel = useMixerStore((s) => s.setInputLevel);
  const fetchMic = useMixerStore((s) => s.fetchMic);
  const [adding, setAdding] = useState<"input" | "mix" | null>(null);
  const [inputKind, setInputKind] = useState<"software" | "hardware">("software");
  const [label, setLabel] = useState("");
  const [icon, setIcon] = useState("generic");
  const [color, setColor] = useState("blue");
  const [device, setDevice] = useState("");
  const [editingMix, setEditingMix] = useState<string | null>(null);
  const [editingInput, setEditingInput] = useState<string | null>(null);
  const [editingEq, setEditingEq] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");

  const mixes = useMemo(() => routing?.mixes ?? buses.map((bus, order) => ({
    id: bus.name, label: bus.label, icon: "broadcast", volume_percent: bus.volume_percent,
    muted: bus.muted, output_bindings: [], order, role: bus.role,
  })), [routing, buses]);
  const inputs = useMemo(() => routing?.inputs ?? channels.map((channel, order) => ({
    id: channel.name, label: channel.label, icon: channel.icon, icon_color: channel.icon_color ?? null,
    kind: "software" as const, source_name: channel.name, volume_percent: channel.volume_percent,
    muted: channel.muted, fx: { high_pass_hz: null, eq_enabled: false, gate_enabled: false, compressor_enabled: false, limiter_enabled: false }, order,
  })), [routing, channels]);

  if (!routing && !channels.length) return <div className="empty-hint">Building mixer…</div>;

  const openAdd = () => { setCreateError(""); void fetchMic(); setAdding("input"); };
  const closeAdd = () => { if (!creating) { setAdding(null); setLabel(""); setDevice(""); setCreateError(""); } };
  const create = async () => {
    const name = label.trim();
    if (!name) return;
    if (adding === "mix") { void addBus(name); closeAdd(); return; }
    setCreating(true);
    const created = inputKind === "software"
      ? await addChannel(name, icon, color)
      : await addHardwareInput(device, name, icon, color);
    setCreating(false);
    if (created) closeAdd();
    else setCreateError("Could not create input. Check audio engine, then try again.");
  };
  const cellFor = (inputId: string, mixId: string) => routing?.routes[inputId]?.[mixId] ?? blankCell;

  return <main className="wave-board">
    <section className="wave-scroll">
      <div className="wave-grid" style={{ gridTemplateColumns: `minmax(288px, 1.25fr) repeat(${mixes.length}, minmax(188px, 1fr)) 52px` }}>
        <div className="wave-corner"><span>SINK MIXER</span></div>
        {mixes.map((mix) => <MixHeader key={mix.id} mix={mix} outputs={outputs} onMute={() => void setBusMute(mix.id, !mix.muted)} onEdit={() => setEditingMix(mix.id)} />)}
        <button type="button" className="wave-add-mix" onClick={() => setAdding("mix")} aria-label="Add mix" title="Add mix"><Ms name="add" /></button>

        {inputs.map((input) => {
          const channel = channels.find((entry) => entry.name === input.id);
          const volume = input.id === "sink_mic" ? (micConfig?.gain_percent ?? input.volume_percent) : (channel?.volume_percent ?? input.volume_percent);
          const muted = input.id === "sink_mic" ? (micConfig?.muted ?? input.muted) : (channel?.muted ?? input.muted);
          const level = levels[input.id];
          const meter = Math.min(100, Math.max(level?.[0] ?? 0, level?.[1] ?? 0) * 100);
          const hardware = input.kind === "hardware";
          return <div className="wave-row" key={input.id}>
            <div className={"wave-input" + (muted ? " muted" : "")} key={`${input.id}:source`}>
              <button type="button" className="wave-input-icon" onContextMenu={(event) => { event.preventDefault(); setEditingInput(input.id); }} title="Right-click to edit input">{CHANNEL_ICON_IDS.includes(input.icon as typeof CHANNEL_ICON_IDS[number]) ? <ChannelIcon id={input.icon} color={input.icon_color} /> : <span className="wave-legacy-icon"><Ms name={channel ? channelIcon(channel) : hardware ? "mic" : "graphic_eq"} /></span>}</button>
              <div className="wave-input-copy"><strong>{input.label}</strong><div className="wave-meter"><i style={{ width: `${meter}%` }} /></div></div>
              <button type="button" className="wave-mute" onClick={() => input.id === "sink_mic" ? void setMicConfig({ muted: !muted }) : hardware ? void setInputLevel(input.id, volume, !muted) : void toggleMute(input.id, !muted)} aria-label={muted ? `Unmute ${input.label}` : `Mute ${input.label}`}><Ms name={muted ? "volume_off" : "volume_up"} /></button>
              <div className="wave-source-slider"><input aria-label={`${input.label} source volume`} type="range" min="0" max={MAX_VOLUME} value={volume} style={{ "--slider-value": `${(volume / MAX_VOLUME) * 100}%` } as CSSProperties} onChange={(event) => input.id === "sink_mic" ? void setMicConfig({ gain_percent: Number(event.target.value) }) : hardware ? void setInputLevel(input.id, Number(event.target.value), muted) : void setChannelVolume(input.id, Number(event.target.value))} /></div>
              <button type="button" className="wave-eq" onClick={() => channel ? setEditingEq(input.id) : setEditingInput(input.id)} title="Equalizer"><Ms name="tune" /></button>
            </div>
            {mixes.map((mix) => <SendCell key={`${input.id}:${mix.id}`} cell={cellFor(input.id, mix.id)} unavailable={false} onChange={(next) => void setRouteCell(input.id, mix.id, next)} />)}
            <div className="wave-grid-spacer" aria-hidden="true" />
          </div>;
        })}

        <button type="button" className="wave-add-input" onClick={openAdd}><Ms name="add" /><span>Add input</span><small>Software or hardware</small></button>
        {mixes.map((mix) => <div className="wave-bottom-spacer" key={`${mix.id}:bottom`} />)}
        <div className="wave-grid-spacer" aria-hidden="true" />
      </div>
    </section>

    <Modal open={adding !== null} onClose={closeAdd} title={adding === "mix" ? "New mix" : "New input"}>
      {adding === "input" && <div className="input-kind"><button type="button" className={inputKind === "software" ? "active" : ""} onClick={() => setInputKind("software")}>Software channel</button><button type="button" className={inputKind === "hardware" ? "active" : ""} onClick={() => { setInputKind("hardware"); void fetchMic(); }}>Hardware source</button></div>}
      {adding === "input" && inputKind === "hardware" && (inputDevices.length ? <select className="menu-input hardware-source-select" value={device} onChange={(event) => setDevice(event.target.value)}><option value="">Select source</option>{inputDevices.map((entry) => <option key={entry.name} value={entry.name}>{entry.description}</option>)}</select> : <p className="modal-text">No hardware inputs detected.</p>)}
      <input className="menu-input" autoFocus placeholder={adding === "mix" ? "Mix name" : "Input name"} value={label} onChange={(event) => setLabel(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") create(); }} />
      {adding === "input" && <><div className="wave-icon-grid">{CHANNEL_ICON_IDS.map((id) => <button type="button" key={id} className={icon === id ? "selected" : ""} onClick={() => setIcon(id)}><ChannelIcon id={id} color={color} /></button>)}</div><div className="icon-color-grid">{CHANNEL_COLORS.map((entry) => <button type="button" key={entry} className={"icon-color-choice icon-color-" + entry + (color === entry ? " sel" : "")} onClick={() => setColor(entry)} aria-label={entry} />)}</div></>}
      {createError && <p className="modal-error" role="alert">{createError}</p>}
      <div className="modal-btns"><button type="button" className="modal-btn primary" disabled={creating || !label.trim() || (adding === "input" && inputKind === "hardware" && !device)} onClick={() => void create()}>{creating ? "Creating…" : "Create"}</button><button type="button" className="modal-btn" disabled={creating} onClick={closeAdd}>Cancel</button></div>
    </Modal>
    {editingMix && <EditMixModal mix={mixes.find((mix) => mix.id === editingMix)!} outputs={outputs} onClose={() => setEditingMix(null)} onOutputs={(names) => void setMixOutputs(editingMix, names)} onRename={(name) => void renameBus(editingMix, name)} onIcon={(nextIcon) => void setBusIcon(editingMix, nextIcon)} onDelete={() => { void removeBus(editingMix); setEditingMix(null); }} />}
    {editingInput && <InputEditModal input={inputs.find((input) => input.id === editingInput)!} onClose={() => setEditingInput(null)} />}
    {editingEq && <EqModal channel={channels.find((channel) => channel.name === editingEq)!} open onClose={() => setEditingEq(null)} />}
  </main>;
}

function MixHeader({ mix, outputs, onMute, onEdit }: Readonly<{ mix: { label: string; icon: string | null; muted: boolean; output_bindings: { device: string; enabled: boolean }[] }; outputs: { name: string; description: string }[]; onMute: () => void; onEdit: () => void }>) {
  const count = mix.output_bindings.filter((binding) => binding.enabled && outputs.some((output) => output.name === binding.device)).length;
  return <div className={"wave-mix-head" + (mix.muted ? " muted" : "")}><div className="wave-mix-label"><button type="button" className="wave-mix-mark" onClick={onMute} onContextMenu={(event) => { event.preventDefault(); onEdit(); }} title="Click to mute. Right-click to edit"><ChannelIcon id={mix.icon ?? null} color={mix.muted ? "gray" : "purple"} /></button><div><strong>{mix.label}</strong>{count > 0 && <small>{count} Output{count === 1 ? "" : "s"}</small>}</div></div></div>;
}

function EditMixModal({ mix, outputs, onClose, onOutputs, onRename, onIcon, onDelete }: Readonly<{ mix: { label: string; icon: string | null; output_bindings: { device: string; enabled: boolean }[] }; outputs: { name: string; description: string }[]; onClose: () => void; onOutputs: (names: string[]) => void; onRename: (name: string) => void; onIcon: (icon: string) => void; onDelete: () => void }>) {
  const [name, setName] = useState(mix.label);
  const [icon, setIcon] = useState(mix.icon ?? "broadcast");
  const selected = mix.output_bindings.filter((binding) => binding.enabled).map((binding) => binding.device);
  return <Modal open onClose={onClose} title="Edit mix"><div className="edit-mix-preview"><ChannelIcon id={icon} color="purple" /><strong>{mix.label}</strong></div><label className="modal-label">Name<input className="menu-input" value={name} maxLength={24} onChange={(event) => setName(event.target.value)} onBlur={() => { if (name.trim() && name.trim() !== mix.label) onRename(name.trim()); }} /></label><div className="modal-label">Icon</div><div className="wave-icon-grid">{CHANNEL_ICON_IDS.map((entry) => <button type="button" key={entry} className={icon === entry ? "selected" : ""} onClick={() => { setIcon(entry); onIcon(entry); }}><ChannelIcon id={entry} color="purple" /></button>)}</div><div className="modal-label">Outputs</div><div className="edit-mix-outputs">{outputs.map((output) => <label key={output.name}><input type="checkbox" checked={selected.includes(output.name)} onChange={(event) => onOutputs(event.target.checked ? [...selected, output.name] : selected.filter((name) => name !== output.name))} /> {output.description}</label>)}</div><div className="modal-btns"><button type="button" className="modal-btn danger" onClick={onDelete}>Delete mix</button><button type="button" className="modal-btn primary" onClick={onClose}>Done</button></div></Modal>;
}

function InputEditModal({ input, onClose }: Readonly<{ input: { label: string; kind: string; source_name: string }; onClose: () => void }>) {
  return <Modal open onClose={onClose} title={`Edit ${input.label}`}><p className="modal-text">{input.kind === "hardware" ? `Hardware source: ${input.source_name}` : "Software channel settings"}</p><div className="modal-btns"><button type="button" className="modal-btn primary" onClick={onClose}>Done</button></div></Modal>;
}

function SendCell({ cell, unavailable, onChange }: Readonly<{ cell: RouteCell; unavailable: boolean; onChange: (cell: RouteCell) => void }>) {
  if (unavailable) return <div className="wave-send unavailable"><Ms name="link_off" /><span>Unavailable</span></div>;
  return <div className={"wave-send" + (!cell.enabled ? " off" : "")}><button type="button" className="wave-send-toggle" onClick={() => onChange({ ...cell, enabled: !cell.enabled })} aria-label={cell.enabled ? "Remove route" : "Add route"}><Ms name={cell.enabled ? "check" : "add"} /></button><div><input aria-label="Mix send level" type="range" min="0" max="150" value={cell.send_percent} style={{ "--slider-value": `${(cell.send_percent / 150) * 100}%` } as CSSProperties} disabled={!cell.enabled} onChange={(event) => onChange({ ...cell, send_percent: Number(event.target.value) })} /></div><button type="button" className={cell.muted ? "wave-send-mute active" : "wave-send-mute"} disabled={!cell.enabled} onClick={() => onChange({ ...cell, muted: !cell.muted })} aria-label={cell.muted ? "Unmute route" : "Mute route"}><Ms name={cell.muted ? "volume_off" : "volume_up"} /></button></div>;
}
