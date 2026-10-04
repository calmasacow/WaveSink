import { useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useMixerStore } from "../../store/mixer";
import { gainDb, peakOf, sendGain, useMeter, useMeterConfig } from "../../lib/meters";
import {
  FX_DEFAULTS,
  MAX_VOLUME,
  SYSTEM_DEFAULT_OUTPUT,
  type FxChain,
  type RouteCell,
} from "../../types";
import { Ms } from "../Icons";
import { Modal } from "../Modal";
import { channelIconId, CHANNEL_COLORS, CHANNEL_ICON_IDS, ChannelIcon } from "../ChannelIcon";
import { EqModal } from "../Eq/EqModal";
import { DspSlider } from "../DspSlider";
import { ToggleRow } from "../Toggle";

const blankCell: RouteCell = { enabled: false, send_percent: 100, muted: false };

export function RoutingTable() {
  const routing = useMixerStore((s) => s.routing);
  const channels = useMixerStore((s) => s.channels);
  const outputs = useMixerStore((s) => s.outputDevices);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const setRouteCell = useMixerStore((s) => s.setRouteCell);
  const setInputFx = useMixerStore((s) => s.setInputFx);
  const setChannelVolume = useMixerStore((s) => s.setChannelVolume);
  const renameChannel = useMixerStore((s) => s.renameChannel);
  const setChannelIcon = useMixerStore((s) => s.setChannelIcon);
  const setChannelIconColor = useMixerStore((s) => s.setChannelIconColor);
  const toggleMute = useMixerStore((s) => s.toggleMute);
  const setMixOutputs = useMixerStore((s) => s.setMixOutputs);
  const setBusMute = useMixerStore((s) => s.setBusMute);
  const setBusVolume = useMixerStore((s) => s.setBusVolume);
  const renameBus = useMixerStore((s) => s.renameBus);
  const setBusIcon = useMixerStore((s) => s.setBusIcon);
  const setBusIconColor = useMixerStore((s) => s.setBusIconColor);
  const removeBus = useMixerStore((s) => s.removeBus);
  const addChannel = useMixerStore((s) => s.addChannel);
  const addBus = useMixerStore((s) => s.addBus);
  const addHardwareInput = useMixerStore((s) => s.addHardwareInput);
  const updateHardwareInput = useMixerStore((s) => s.updateHardwareInput);
  const removeHardwareInput = useMixerStore((s) => s.removeHardwareInput);
  const removeChannel = useMixerStore((s) => s.removeChannel);
  const setInputLevel = useMixerStore((s) => s.setInputLevel);
  const toggleSolo = useMixerStore((s) => s.toggleSolo);
  const fetchInputDevices = useMixerStore((s) => s.fetchInputDevices);
  const [adding, setAdding] = useState<"input" | "mix" | null>(null);
  const [inputKind, setInputKind] = useState<"software" | "hardware">("software");
  const [label, setLabel] = useState("");
  const [icon, setIcon] = useState("generic");
  const [color, setColor] = useState("blue");
  const [device, setDevice] = useState("");
  const [editingMix, setEditingMix] = useState<string | null>(null);
  const [editingInput, setEditingInput] = useState<string | null>(null);
  const [editingEq, setEditingEq] = useState<string | null>(null);
  const [editingFx, setEditingFx] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");
  const [inputOrder, setInputOrder] = useState<string[] | null>(null);
  const [mixOrder, setMixOrder] = useState<string[] | null>(null);
  const [reordering, setReordering] = useState<{ kind: "input" | "mix"; id: string } | null>(null);
  const hold = useRef<number | null>(null);

  const mixes = useMemo(() => routing?.mixes ?? [], [routing]);
  const inputs = useMemo(
    () =>
      routing?.inputs ??
      channels.map((channel, order) => ({
        id: channel.name,
        label: channel.label,
        icon: channel.icon,
        icon_color: channel.icon_color ?? null,
        kind: "software" as const,
        source_name: channel.name,
        volume_percent: channel.volume_percent,
        muted: channel.muted,
        fx: {
          high_pass_hz: null,
          eq_enabled: false,
          gate_enabled: false,
          compressor_enabled: false,
          limiter_enabled: false,
          ...FX_DEFAULTS,
        },
        order,
      })),
    [routing, channels],
  );
  const editedInput = editingInput ? inputs.find((input) => input.id === editingInput) : undefined;
  const orderedInputs = inputOrder
    ? [...inputs].sort((a, b) => inputOrder.indexOf(a.id) - inputOrder.indexOf(b.id))
    : inputs;
  const orderedMixes = mixOrder
    ? [...mixes].sort((a, b) => mixOrder.indexOf(a.id) - mixOrder.indexOf(b.id))
    : mixes;

  const stopReorder = () => {
    if (hold.current !== null) {
      window.clearTimeout(hold.current);
      hold.current = null;
    }
    if (!reordering) return;
    const order = reordering.kind === "input" ? inputOrder : mixOrder;
    const command = reordering.kind === "input" ? "reorder_matrix_inputs" : "reorder_matrix_mixes";
    setReordering(null);
    if (order)
      void invoke(command, { order }).catch((error) => {
        useMixerStore.setState({ error: String(error) });
        void useMixerStore.getState().fetchRouting();
      });
  };
  useEffect(
    () => () => {
      if (hold.current !== null) window.clearTimeout(hold.current);
    },
    [],
  );
  useEffect(() => {
    window.addEventListener("pointerup", stopReorder);
    return () => window.removeEventListener("pointerup", stopReorder);
  });
  const beginHold = (kind: "input" | "mix", id: string, event: PointerEvent<HTMLElement>) => {
    if ((event.target as HTMLElement).closest("button, input, select, label")) return;
    hold.current = window.setTimeout(() => {
      hold.current = null;
      setReordering({ kind, id });
      if (kind === "input") setInputOrder(orderedInputs.map((input) => input.id));
      else setMixOrder(orderedMixes.map((mix) => mix.id));
    }, 2000);
  };
  const moveReorder = (kind: "input" | "mix", target: string) => {
    if (!reordering || reordering.kind !== kind || reordering.id === target) return;
    const move = (order: string[]) => {
      const next = [...order];
      const from = next.indexOf(reordering.id);
      next.splice(from, 1);
      next.splice(next.indexOf(target), 0, reordering.id);
      return next;
    };
    if (kind === "input")
      setInputOrder((order) => move(order ?? orderedInputs.map((input) => input.id)));
    else setMixOrder((order) => move(order ?? orderedMixes.map((mix) => mix.id)));
  };

  if (!routing && !channels.length) return <div className="empty-hint">Building mixer…</div>;

  const openAdd = () => {
    setCreateError("");
    void fetchInputDevices();
    setAdding("input");
  };
  const closeAdd = () => {
    if (!creating) {
      setAdding(null);
      setLabel("");
      setDevice("");
      setCreateError("");
    }
  };
  const create = async () => {
    const name = label.trim();
    if (!name) return;
    if (adding === "mix") {
      void addBus(name);
      closeAdd();
      return;
    }
    setCreating(true);
    const created =
      inputKind === "software"
        ? await addChannel(name, icon, color)
        : await addHardwareInput(device, name, icon, color);
    setCreating(false);
    if (created) closeAdd();
    else setCreateError("Could not create input. Check audio engine, then try again.");
  };
  const cellFor = (inputId: string, mixId: string) =>
    routing?.routes[inputId]?.[mixId] ?? blankCell;

  return (
    <main className="wave-board">
      <section className="wave-scroll">
        <div
          className={"wave-grid" + (reordering ? " reordering" : "")}
          style={{
            gridTemplateColumns: `minmax(288px, 1.25fr) repeat(${orderedMixes.length}, minmax(188px, 1fr)) 52px`,
          }}
        >
          <div className="wave-corner">
            <span>MIXER</span>
          </div>
          {orderedMixes.map((mix) => (
            <MixHeader
              key={mix.id}
              mix={mix}
              volume={mix.volume_percent}
              outputs={outputs}
              reordering={reordering?.kind === "mix" && reordering.id === mix.id}
              onHold={(event) => beginHold("mix", mix.id, event)}
              onEnter={() => moveReorder("mix", mix.id)}
              onMute={() => void setBusMute(mix.id, !mix.muted)}
              onVolume={(volume) => void setBusVolume(mix.id, volume)}
              onEdit={() => setEditingMix(mix.id)}
            />
          ))}
          <button
            type="button"
            className="wave-add-mix"
            onClick={() => setAdding("mix")}
            aria-label="Add mix"
            title="Add mix"
          >
            <Ms name="add" />
          </button>

          {orderedInputs.map((input) => {
            const channel = channels.find((entry) => entry.name === input.id);
            const volume = channel?.volume_percent ?? input.volume_percent;
            const muted = channel?.muted ?? input.muted;
            const hardware = input.kind === "hardware";
            // Channels meter on their own sink. A hardware input meters what its
            // mixes receive: the processed stream while Audio FX is on.
            const meterKey = !hardware
              ? input.id
              : fxActive(input.fx)
                ? `fx:${input.id}`
                : input.source_name;
            const inputLevel = () => (muted ? 0 : peakOf(meterKey));
            return (
              <div className="wave-row" key={input.id}>
                <div
                  className={
                    "wave-input" +
                    (muted ? " muted" : "") +
                    (reordering?.kind === "input" && reordering.id === input.id
                      ? " reorder-active"
                      : "")
                  }
                  key={`${input.id}:source`}
                  onPointerDown={(event) => beginHold("input", input.id, event)}
                  onPointerEnter={() => moveReorder("input", input.id)}
                >
                  <button
                    type="button"
                    className={
                      "wave-input-icon" + (routing?.solo?.input === input.id ? " soloed" : "")
                    }
                    onClick={() => setEditingInput(input.id)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      void toggleSolo(input.id);
                    }}
                    title={
                      routing?.solo?.input === input.id
                        ? "Edit input · right-click to un-solo"
                        : "Edit input · right-click to solo"
                    }
                  >
                    <ChannelIcon id={input.icon} color={input.icon_color} />
                  </button>
                  <div className="wave-input-copy">
                    <strong>{input.label}</strong>
                  </div>
                  <button
                    type="button"
                    className="wave-mute"
                    onClick={() =>
                      hardware
                        ? void setInputLevel(input.id, volume, !muted)
                        : void toggleMute(input.id, !muted)
                    }
                    aria-label={muted ? `Unmute ${input.label}` : `Mute ${input.label}`}
                  >
                    <Ms name={muted ? "volume_off" : "volume_up"} />
                  </button>
                  <div className="wave-source-slider">
                    <MeterSlider
                      label={`${input.label} source volume`}
                      value={volume}
                      level={inputLevel}
                      onChange={(value) =>
                        hardware
                          ? void setInputLevel(input.id, value, muted)
                          : void setChannelVolume(input.id, value)
                      }
                    />
                  </div>
                  <button
                    type="button"
                    className="wave-eq"
                    onClick={() => (channel ? setEditingEq(input.id) : setEditingFx(input.id))}
                    title={channel ? "Equalizer" : "Audio FX"}
                  >
                    <Ms name="tune" />
                  </button>
                </div>
                {orderedMixes.map((mix) => (
                  <SendCell
                    key={`${input.id}:${mix.id}`}
                    cell={cellFor(input.id, mix.id)}
                    inputLevel={inputLevel}
                    unavailable={false}
                    onChange={(next) => void setRouteCell(input.id, mix.id, next)}
                  />
                ))}
                <div className="wave-grid-spacer" aria-hidden="true" />
              </div>
            );
          })}

          <button type="button" className="wave-add-input" onClick={openAdd}>
            <Ms name="add" />
            <span>Add input</span>
            <small>Software or hardware</small>
          </button>
          {orderedMixes.map((mix) => (
            <div className="wave-bottom-spacer" key={`${mix.id}:bottom`} />
          ))}
          <div className="wave-grid-spacer" aria-hidden="true" />
        </div>
      </section>

      <Modal
        open={adding !== null}
        onClose={closeAdd}
        title={adding === "mix" ? "New mix" : "New input"}
      >
        {adding === "input" && (
          <div className="input-kind">
            <button
              type="button"
              className={inputKind === "software" ? "active" : ""}
              onClick={() => setInputKind("software")}
            >
              Software channel
            </button>
            <button
              type="button"
              className={inputKind === "hardware" ? "active" : ""}
              onClick={() => {
                setInputKind("hardware");
                void fetchInputDevices();
              }}
            >
              Hardware source
            </button>
          </div>
        )}
        {adding === "input" &&
          inputKind === "hardware" &&
          (inputDevices.filter((entry) => !entry.name.startsWith("sink_")).length ? (
            <select
              className="menu-input hardware-source-select"
              value={device}
              onChange={(event) => setDevice(event.target.value)}
            >
              <option value="">Select source</option>
              {inputDevices
                .filter((entry) => !entry.name.startsWith("sink_"))
                .map((entry) => (
                  <option key={entry.name} value={entry.name}>
                    {entry.description}
                  </option>
                ))}
            </select>
          ) : (
            <p className="modal-text">No hardware inputs detected.</p>
          ))}
        <input
          className="menu-input"
          autoFocus
          placeholder={adding === "mix" ? "Mix name" : "Input name"}
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") create();
          }}
        />
        {adding === "input" && (
          <>
            <div className="wave-icon-grid">
              {CHANNEL_ICON_IDS.map((id) => (
                <button
                  type="button"
                  key={id}
                  className={icon === id ? "selected" : ""}
                  onClick={() => setIcon(id)}
                >
                  <ChannelIcon id={id} color={color} />
                </button>
              ))}
            </div>
            <div className="icon-color-grid">
              {CHANNEL_COLORS.map((entry) => (
                <button
                  type="button"
                  key={entry}
                  className={
                    "icon-color-choice icon-color-" + entry + (color === entry ? " sel" : "")
                  }
                  onClick={() => setColor(entry)}
                  aria-label={entry}
                />
              ))}
            </div>
          </>
        )}
        {createError && (
          <p className="modal-error" role="alert">
            {createError}
          </p>
        )}
        <div className="modal-btns">
          <button
            type="button"
            className="modal-btn primary"
            disabled={
              creating ||
              !label.trim() ||
              (adding === "input" && inputKind === "hardware" && !device)
            }
            onClick={() => void create()}
          >
            {creating ? "Creating…" : "Create"}
          </button>
          <button type="button" className="modal-btn" disabled={creating} onClick={closeAdd}>
            Cancel
          </button>
        </div>
      </Modal>
      {editingMix && (
        <EditMixModal
          mix={mixes.find((mix) => mix.id === editingMix)!}
          outputs={outputs}
          onClose={() => setEditingMix(null)}
          onOutputs={(names) => void setMixOutputs(editingMix, names)}
          onRename={(name) => void renameBus(editingMix, name)}
          onIcon={(nextIcon) => void setBusIcon(editingMix, nextIcon)}
          onColor={(nextColor) => void setBusIconColor(editingMix, nextColor)}
          onDelete={() => {
            void removeBus(editingMix);
            setEditingMix(null);
          }}
        />
      )}
      {editedInput && (
        <InputEditModal
          input={editedInput}
          devices={inputDevices}
          onRefreshDevices={fetchInputDevices}
          onClose={() => setEditingInput(null)}
          onHardwareSave={updateHardwareInput}
          onRemoveHardware={removeHardwareInput}
          onSetFx={setInputFx}
          onSoftwareSave={async (id, name, nextIcon, nextColor) => {
            await renameChannel(id, name);
            await setChannelIcon(id, nextIcon);
            await setChannelIconColor(id, nextColor);
            await useMixerStore.getState().fetchRouting();
          }}
          onRemoveSoftware={removeChannel}
        />
      )}
      {editingEq && (
        <EqModal
          channel={channels.find((channel) => channel.name === editingEq)!}
          open
          onClose={() => setEditingEq(null)}
        />
      )}
      {editingFx && (
        <AudioFxModal
          input={inputs.find((input) => input.id === editingFx)!}
          onClose={() => setEditingFx(null)}
          onChange={(fx) => void setInputFx(editingFx, fx)}
        />
      )}
    </main>
  );
}

function MixHeader({
  mix,
  volume,
  outputs,
  reordering,
  onHold,
  onEnter,
  onMute,
  onVolume,
  onEdit,
}: Readonly<{
  mix: {
    id: string;
    label: string;
    icon: string | null;
    icon_color: string | null;
    muted: boolean;
    output_bindings: { device: string; enabled: boolean }[];
  };
  volume: number;
  outputs: { name: string; description: string }[];
  reordering: boolean;
  onHold: (event: PointerEvent<HTMLDivElement>) => void;
  onEnter: () => void;
  onMute: () => void;
  onVolume: (volume: number) => void;
  onEdit: () => void;
}>) {
  const { pro } = useMeterConfig();
  const count = mix.output_bindings.filter(
    (binding) =>
      binding.enabled &&
      (binding.device === SYSTEM_DEFAULT_OUTPUT ||
        outputs.some((output) => output.name === binding.device)),
  ).length;
  const color = mix.muted ? "gray" : (mix.icon_color ?? "purple");
  return (
    <div
      className={"wave-mix-head mix-color-" + color + (reordering ? " reorder-active" : "")}
      onPointerDown={onHold}
      onPointerEnter={onEnter}
    >
      <div className="wave-mix-label">
        <button type="button" className="wave-mix-mark" onClick={onEdit} title="Edit mix">
          <ChannelIcon id={mix.icon ?? null} color={color} />
        </button>
        <div>
          <strong>{mix.label}</strong>
          {count > 0 && (
            <small>
              {count} Output{count === 1 ? "" : "s"}
            </small>
          )}
        </div>
      </div>
      <button
        type="button"
        className="wave-mix-mute"
        onClick={onMute}
        aria-label={mix.muted ? `Unmute ${mix.label}` : `Mute ${mix.label}`}
        title={mix.muted ? "Unmute mix" : "Mute mix"}
      >
        <Ms name={mix.muted ? "volume_off" : "volume_up"} />
      </button>
      {/* The mix's one volume control: recorders and every output this mix
          plays to hear it. Pointer events stay here so dragging never starts
          the header's hold-to-reorder. */}
      <div
        className="wave-source-slider wave-mix-level"
        onPointerDown={(event) => event.stopPropagation()}
      >
        <MeterSlider
          label={`${mix.label} mix volume`}
          value={volume}
          level={() => (mix.muted ? 0 : peakOf(mix.id))}
          onChange={onVolume}
        />
        <span>{pro ? gainDb(volume) : `${volume}%`}</span>
      </div>
    </div>
  );
}

function EditMixModal({
  mix,
  outputs,
  onClose,
  onOutputs,
  onRename,
  onIcon,
  onColor,
  onDelete,
}: Readonly<{
  mix: {
    label: string;
    icon: string | null;
    icon_color: string | null;
    output_bindings: { device: string; enabled: boolean }[];
  };
  outputs: { name: string; description: string }[];
  onClose: () => void;
  onOutputs: (names: string[]) => void;
  onRename: (name: string) => void;
  onIcon: (icon: string) => void;
  onColor: (color: string) => void;
  onDelete: () => void;
}>) {
  const [name, setName] = useState(mix.label);
  const [icon, setIcon] = useState(channelIconId(mix.icon ?? "broadcast"));
  const [color, setColor] = useState(mix.icon_color ?? "purple");
  const selected = mix.output_bindings
    .filter((binding) => binding.enabled)
    .map((binding) => binding.device);
  return (
    <Modal open onClose={onClose} title="Edit mix">
      <div className="edit-mix-preview">
        <ChannelIcon id={icon} color={color} />
        <strong>{mix.label}</strong>
      </div>
      <label className="modal-label">
        Name
        <input
          className="menu-input"
          value={name}
          maxLength={24}
          onChange={(event) => setName(event.target.value)}
          onBlur={() => {
            if (name.trim() && name.trim() !== mix.label) onRename(name.trim());
          }}
        />
      </label>
      <div className="modal-label">Icon</div>
      <div className="wave-icon-grid">
        {CHANNEL_ICON_IDS.map((entry) => (
          <button
            type="button"
            key={entry}
            className={icon === entry ? "selected" : ""}
            onClick={() => {
              setIcon(entry);
              onIcon(entry);
            }}
          >
            <ChannelIcon id={entry} color={color} />
          </button>
        ))}
      </div>
      <div className="modal-label">Icon color</div>
      <div className="icon-color-grid">
        {CHANNEL_COLORS.map((entry) => (
          <button
            type="button"
            key={entry}
            className={"icon-color-choice icon-color-" + entry + (color === entry ? " sel" : "")}
            onClick={() => {
              setColor(entry);
              onColor(entry);
            }}
            aria-label={entry}
          />
        ))}
      </div>
      <div className="modal-label">Outputs</div>
      <div className="edit-mix-outputs">
        {/* Channels reach speakers only through mixes; "System default" follows
            whatever the desktop's default output is, e.g. headphones when plugged in. */}
        {[{ name: SYSTEM_DEFAULT_OUTPUT, description: "System default" }, ...outputs].map(
          (output) => (
            <label key={output.name}>
              <input
                type="checkbox"
                checked={selected.includes(output.name)}
                onChange={(event) =>
                  onOutputs(
                    event.target.checked
                      ? [...selected, output.name]
                      : selected.filter((name) => name !== output.name),
                  )
                }
              />{" "}
              {output.description}
            </label>
          ),
        )}
      </div>
      <div className="modal-btns">
        <button type="button" className="modal-btn danger" onClick={onDelete}>
          Delete mix
        </button>
        <button type="button" className="modal-btn primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Modal>
  );
}

function InputEditModal({
  input,
  devices,
  onRefreshDevices,
  onClose,
  onHardwareSave,
  onRemoveHardware,
  onSetFx,
  onSoftwareSave,
  onRemoveSoftware,
}: Readonly<{
  input: {
    id: string;
    label: string;
    icon: string | null;
    icon_color: string | null;
    kind: string;
    source_name: string;
    fx: FxChain;
  };
  devices: { name: string; description: string }[];
  onRefreshDevices: () => Promise<void>;
  onClose: () => void;
  onHardwareSave: (
    id: string,
    label: string,
    icon: string,
    color: string,
    source: string,
  ) => Promise<boolean>;
  onRemoveHardware: (id: string) => Promise<boolean>;
  onSetFx: (id: string, fx: FxChain) => Promise<void>;
  onSoftwareSave: (id: string, label: string, icon: string, color: string) => Promise<void>;
  onRemoveSoftware: (id: string) => Promise<void>;
}>) {
  const [name, setName] = useState(input.label);
  const [icon, setIcon] = useState(
    channelIconId(input.icon ?? (input.kind === "hardware" ? "mic" : "generic")),
  );
  const [color, setColor] = useState(input.icon_color ?? "blue");
  const [source, setSource] = useState(input.source_name);
  const [editingFx, setEditingFx] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const remove = async () => {
    if (!window.confirm(`Remove ${input.label}?`)) return;
    setSaving(true);
    setError("");
    try {
      const ok =
        input.kind === "hardware"
          ? await onRemoveHardware(input.id)
          : (await onRemoveSoftware(input.id), true);
      if (ok) onClose();
      else setError("Could not remove input.");
    } catch (e) {
      setError(String(e));
    }
    setSaving(false);
  };
  const save = async () => {
    if (!name.trim()) return;
    setSaving(true);
    setError("");
    const ok =
      input.kind === "hardware"
        ? await onHardwareSave(input.id, name.trim(), icon, color, source)
        : (await onSoftwareSave(input.id, name.trim(), icon, color), true);
    setSaving(false);
    if (ok) onClose();
    else setError("Could not update input. Check selected device, then try again.");
  };
  const hardwareDevices = devices.filter((device) => !device.name.startsWith("sink_"));
  return (
    <>
      <Modal open onClose={onClose} title={`Edit ${input.label}`}>
        <div className="edit-mix-preview">
          <ChannelIcon id={icon} color={color} />
          <strong>{name || input.label}</strong>
          {input.kind === "hardware" && (
            <button
              type="button"
              className="modal-btn audio-fx-btn"
              onClick={() => setEditingFx(true)}
            >
              Audio FX
            </button>
          )}
        </div>
        <label className="modal-label">
          Name
          <input
            className="menu-input"
            autoFocus
            value={name}
            maxLength={24}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        {input.kind === "hardware" && (
          <label className="modal-label">
            Hardware source
            <select
              className="menu-input hardware-source-select"
              value={source}
              onFocus={() => void onRefreshDevices()}
              onChange={(event) => setSource(event.target.value)}
            >
              {hardwareDevices.map((device) => (
                <option key={device.name} value={device.name}>
                  {device.description}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="modal-label">Icon</div>
        <div className="wave-icon-grid">
          {CHANNEL_ICON_IDS.map((entry) => (
            <button
              type="button"
              key={entry}
              className={icon === entry ? "selected" : ""}
              onClick={() => setIcon(entry)}
            >
              <ChannelIcon id={entry} color={color} />
            </button>
          ))}
        </div>
        <div className="modal-label">Icon color</div>
        <div className="icon-color-grid">
          {CHANNEL_COLORS.map((entry) => (
            <button
              type="button"
              key={entry}
              className={"icon-color-choice icon-color-" + entry + (color === entry ? " sel" : "")}
              onClick={() => setColor(entry)}
              aria-label={entry}
            />
          ))}
        </div>
        {error && (
          <p className="modal-error" role="alert">
            {error}
          </p>
        )}
        <div className="modal-btns">
          <button
            type="button"
            className="modal-btn danger"
            disabled={saving}
            onClick={() => void remove()}
          >
            Remove input
          </button>
          <button
            type="button"
            className="modal-btn primary"
            disabled={saving || !name.trim() || (input.kind === "hardware" && !source)}
            onClick={() => void save()}
          >
            {saving ? "Saving…" : "Done"}
          </button>
          <button type="button" className="modal-btn" disabled={saving} onClick={onClose}>
            Cancel
          </button>
        </div>
      </Modal>
      {editingFx && (
        <AudioFxModal
          input={input}
          onClose={() => setEditingFx(false)}
          onChange={(fx) => void onSetFx(input.id, fx)}
        />
      )}
    </>
  );
}

function AudioFxModal({
  input,
  onClose,
  onChange,
}: Readonly<{
  input: { label: string; fx: FxChain };
  onClose: () => void;
  onChange: (fx: FxChain) => void;
}>) {
  const fx = input.fx;
  const patch = (next: Partial<FxChain>) => onChange({ ...fx, ...next });
  return (
    <Modal open onClose={onClose} title={`${input.label} Audio FX`} className="audio-fx-modal">
      <p className="modal-text">
        Settings save per input. Live hardware processing is not available yet.
      </p>
      <div className="card" style={{ padding: "var(--sp-2)" }}>
        <ToggleRow
          icon="noise_control_off"
          title="Noise gate"
          sub="Cuts noise between sounds"
          on={fx.gate_enabled}
          onToggle={() => patch({ gate_enabled: !fx.gate_enabled })}
        />
        {fx.gate_enabled && (
          <DspSlider
            label="Threshold"
            min={-80}
            max={-10}
            step={1}
            unit=" dB"
            value={fx.gate_threshold_db}
            defaultValue={FX_DEFAULTS.gate_threshold_db}
            onChange={(gate_threshold_db) => patch({ gate_threshold_db })}
          />
        )}
        <ToggleRow
          icon="compress"
          title="Compressor"
          sub="Evens loud peaks and quiet sound"
          on={fx.compressor_enabled}
          onToggle={() => patch({ compressor_enabled: !fx.compressor_enabled })}
        />
        {fx.compressor_enabled && (
          <>
            <DspSlider
              label="Threshold"
              min={-60}
              max={0}
              step={1}
              unit=" dB"
              value={fx.compressor_threshold_db}
              defaultValue={FX_DEFAULTS.compressor_threshold_db}
              onChange={(compressor_threshold_db) => patch({ compressor_threshold_db })}
            />
            <DspSlider
              label="Ratio"
              min={1}
              max={10}
              step={0.5}
              unit=":1"
              value={fx.compressor_ratio}
              defaultValue={FX_DEFAULTS.compressor_ratio}
              onChange={(compressor_ratio) => patch({ compressor_ratio })}
            />
          </>
        )}
        <ToggleRow
          icon="vertical_align_center"
          title="Limiter"
          sub="Hard ceiling to prevent clipping"
          on={fx.limiter_enabled}
          onToggle={() => patch({ limiter_enabled: !fx.limiter_enabled })}
        />
        {fx.limiter_enabled && (
          <DspSlider
            label="Ceiling"
            min={-12}
            max={0}
            step={0.5}
            unit=" dB"
            value={fx.limiter_ceiling_db}
            defaultValue={FX_DEFAULTS.limiter_ceiling_db}
            onChange={(limiter_ceiling_db) => patch({ limiter_ceiling_db })}
          />
        )}
      </div>
      <div className="modal-btns">
        <button type="button" className="modal-btn primary" onClick={onClose}>
          Done
        </button>
      </div>
    </Modal>
  );
}

function SendCell({
  cell,
  inputLevel,
  unavailable,
  onChange,
}: Readonly<{
  cell: RouteCell;
  /** The input's live level; the cell shows it after its own send gain. */
  inputLevel: () => number;
  unavailable: boolean;
  onChange: (cell: RouteCell) => void;
}>) {
  if (unavailable)
    return (
      <div className="wave-send unavailable">
        <Ms name="link_off" />
        <span>Unavailable</span>
      </div>
    );
  return (
    <div className={"wave-send" + (!cell.enabled ? " off" : "")}>
      <button
        type="button"
        className="wave-send-toggle"
        onClick={() => onChange({ ...cell, enabled: !cell.enabled })}
        aria-label={cell.enabled ? "Remove route" : "Add route"}
      >
        <Ms name={cell.enabled ? "check" : "add"} />
      </button>
      <div>
        <MeterSlider
          label="Mix send level"
          value={cell.send_percent}
          level={() =>
            cell.enabled && !cell.muted ? inputLevel() * sendGain(cell.send_percent) : 0
          }
          disabled={!cell.enabled}
          onChange={(send_percent) => onChange({ ...cell, send_percent })}
        />
      </div>
      <button
        type="button"
        className={cell.muted ? "wave-send-mute active" : "wave-send-mute"}
        disabled={!cell.enabled}
        onClick={() => onChange({ ...cell, muted: !cell.muted })}
        aria-label={cell.muted ? "Unmute route" : "Mute route"}
      >
        <Ms name={cell.muted ? "volume_off" : "volume_up"} />
      </button>
    </div>
  );
}

/** A level slider with its live meter drawn inside the track (`--meter`).
 * In Pro mode, hovering shows the held peak in dBFS. */
function MeterSlider({
  label,
  value,
  level,
  disabled,
  onChange,
}: Readonly<{
  label: string;
  value: number;
  /** Linear peak amplitude to show, read every animation frame. */
  level: () => number;
  disabled?: boolean;
  onChange: (value: number) => void;
}>) {
  const readout = useRef<HTMLDivElement>(null);
  const ref = useMeter<HTMLInputElement>(level, readout);
  return (
    <div className="meter-wrap">
      <input
        ref={ref}
        className="meter-slider"
        aria-label={label}
        type="range"
        min="0"
        max={MAX_VOLUME}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <div className="meter-readout" ref={readout} aria-hidden="true" />
    </div>
  );
}

/** Whether any Audio FX stage is on (mirrors FxChain::is_active). */
function fxActive(fx: FxChain): boolean {
  return fx.gate_enabled || fx.compressor_enabled || fx.limiter_enabled;
}
