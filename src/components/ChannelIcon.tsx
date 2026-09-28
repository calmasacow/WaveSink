import type { CSSProperties } from "react";

export const CHANNEL_ICON_IDS = [
  "game",
  "chat",
  "music",
  "system",
  "mic",
  "broadcast",
  "browser",
  "terminal",
  "star",
  "generic",
  "headphones",
  "public",
  "webcam",
  "camera",
  "forum",
] as const;
export const CHANNEL_COLORS = [
  "slate",
  "gray",
  "red",
  "orange",
  "amber",
  "yellow",
  "lime",
  "green",
  "teal",
  "cyan",
  "blue",
  "purple",
] as const;
type IconId = (typeof CHANNEL_ICON_IDS)[number];

const paths: Record<IconId, string> = {
  game: "M7 8h10l2 4v5a2 2 0 0 1-2 2l-3-2H10l-3 2a2 2 0 0 1-2-2v-5l2-4Zm2.5 4.5v3m-1.5-1.5h3m5.5-1.5h.01m-2 2h.01",
  chat: "M5 5h14v10H9l-4 4V5Zm4 5h.01m3 0h.01m3 0h.01",
  music: "M9 18V6l10-2v12M9 18a2 2 0 1 1-2-2 2 2 0 0 1 2 2Zm10-2a2 2 0 1 1-2-2 2 2 0 0 1 2 2Z",
  system: "M4 5h16v11H4V5Zm6 14h4M8 20h8",
  mic: "M12 14a3 3 0 0 0 3-3V6a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3Zm-5-3a5 5 0 0 0 10 0m-5 5v4m-3 0h6",
  broadcast: "M5 15V9l10-4v14L5 15Zm12-5a3 3 0 0 1 0 4m2-6a6 6 0 0 1 0 8",
  browser: "M4 5h16v14H4V5Zm0 4h16M7 7h.01m3 0h.01m3 0h.01",
  terminal: "M5 5h14v14H5V5Zm3 4 3 3-3 3m5 0h3",
  star: "m12 4 2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4-3.9-3.8 5.4-.8L12 4Z",
  generic: "M12 4v16M4 12h16",
  headphones: "M4 14v-2a8 8 0 0 1 16 0v2m-16 0v4h4v-5H4m16 1v4h-4v-5h4",
  public: "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16",
  webcam: "M6 7h12v10H6V7Zm3 13h6m-3-3v3m-1.5-9.5a1.5 1.5 0 1 0 3 0 1.5 1.5 0 0 0-3 0",
  camera: "M4 8h4l1.5-2h5L16 8h4v10H4V8Zm8 2a3 3 0 1 0 0 6 3 3 0 0 0 0-6",
  forum: "M4 5h12v9H9l-4 4V5Zm6 11h6l4 3v-9h-2",
};

const legacyIds: Record<string, IconId> = {
  sports_esports: "game",
  forum: "forum",
  music_note: "music",
  desktop_windows: "system",
  headphones: "headphones",
  globe: "public",
  public: "public",
};

export function channelIconId(id: string | null): IconId {
  return (
    legacyIds[id ?? ""] ?? (CHANNEL_ICON_IDS.includes(id as IconId) ? (id as IconId) : "generic")
  );
}

export function ChannelIcon({
  id,
  color,
  style,
}: Readonly<{ id: string | null; color?: string | null; style?: CSSProperties }>) {
  const icon = channelIconId(id);
  if (icon === "public")
    return (
      <span className={`channel-svg-icon icon-color-${color ?? "blue"}`} style={style}>
        <span className="ms material-symbols-outlined">public</span>
      </span>
    );
  return (
    <span className={`channel-svg-icon icon-color-${color ?? "blue"}`} style={style}>
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.9"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d={paths[icon]} />
      </svg>
    </span>
  );
}
