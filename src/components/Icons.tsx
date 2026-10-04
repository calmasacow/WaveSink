import { channelIconId } from "./ChannelIcon";
import type { CSSProperties } from "react";

/** Material Symbol glyph (self-hosted via the material-symbols package). */
export function Ms({
  name,
  className,
  style,
}: Readonly<{
  name: string;
  className?: string;
  style?: CSSProperties;
}>) {
  return (
    <span
      className={"ms material-symbols-outlined" + (className ? " " + className : "")}
      style={style}
      aria-hidden="true"
    >
      {name}
    </span>
  );
}

/** Legacy vector fallback for compact windows. */
export function WaveSinkMark() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.1"
      strokeLinecap="round"
    >
      <path
        d="M3 5 L21 5 L13.5 13 L13.5 20 L10.5 20 L10.5 13 Z"
        fill="currentColor"
        stroke="none"
        opacity="0.92"
      />
    </svg>
  );
}

/** Legacy fallback icons for channels created before icons existed. */
export const CHANNEL_ICONS: Record<string, string> = {
  sink_game: "sports_esports",
  sink_chat: "forum",
  sink_music: "music_note",
  sink_system: "desktop_windows",
};

/** The Material Symbols glyph for each bundled channel icon, so places that
 *  draw a channel's icon with the font (menus, the Apps screen) match the
 *  bundled SVG the mixer shows. */
const MATERIAL_FOR_CHANNEL_ICON: Record<ReturnType<typeof channelIconId>, string> = {
  game: "sports_esports",
  chat: "chat",
  music: "music_note",
  system: "desktop_windows",
  mic: "mic",
  broadcast: "podcasts",
  browser: "web",
  terminal: "terminal",
  star: "star",
  generic: "graphic_eq",
  headphones: "headphones",
  public: "public",
  webcam: "videocam",
  camera: "photo_camera",
  forum: "forum",
};

/** A channel's icon as a Material Symbols glyph name. Stored icons are either
 *  a bundled id ("music") or an older Material name ("music_note"); both
 *  resolve through `channelIconId`, so a bundled id never renders as text. */
export function channelIcon(channel: { name: string; icon?: string | null }): string {
  const stored = channel.icon ?? CHANNEL_ICONS[channel.name] ?? null;
  return MATERIAL_FOR_CHANNEL_ICON[channelIconId(stored)];
}

/** Curated icon choices for the channel icon picker. */
export const ICON_CHOICES: string[] = [
  "sports_esports",
  "forum",
  "music_note",
  "desktop_windows",
  "headphones",
  "mic",
  "movie",
  "tv",
  "videogame_asset",
  "campaign",
  "record_voice_over",
  "radio",
  "podcasts",
  "terminal",
  "public",
  "star",
];
