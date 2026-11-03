import { useEffect, useState } from "react";
import { Modal } from "../Modal";
import { alerting, useUpdate } from "../../store/update";

/** "2 hours ago", for the tooltip. */
function sinceLabel(checkedAt: number | null): string {
  if (!checkedAt) return "not checked yet";
  const minutes = Math.max(0, Math.round((Date.now() / 1000 - checkedAt) / 60));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  return hours === 1 ? "1 hour ago" : `${hours} hours ago`;
}

/**
 * Update status beside the engine pill: muted when current, accent-coloured
 * when a release is waiting. Clicking an available update opens the dialog;
 * clicking an up-to-date pill re-checks.
 *
 * Only an AppImage can install from here - see commands/update.rs. Packaged
 * installs need root, so they get the exact command instead.
 */
export function UpdatePill() {
  const status = useUpdate((s) => s.status);
  const installing = useUpdate((s) => s.installing);
  const error = useUpdate((s) => s.error);
  const check = useUpdate((s) => s.check);
  const install = useUpdate((s) => s.install);
  const launch = useUpdate((s) => s.launch);
  const skipped = useUpdate((s) => s.skipped);
  const setSkipped = useUpdate((s) => s.setSkipped);
  const [open, setOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const [copied, setCopied] = useState(false);
  // Mirrors the stored choice so the box can be ticked before the dialog is
  // dismissed; re-synced whenever a different version comes along.
  const [skipThis, setSkipThis] = useState(false);

  const latestTag = status?.latest ?? null;
  useEffect(() => {
    setSkipThis(latestTag !== null && latestTag === skipped);
  }, [latestTag, skipped]);

  // Nothing to show until the first check returns.
  if (!status) return null;

  const {
    available,
    latest,
    current,
    command,
    notes_url,
    can_self_update,
    can_launch_update,
    install: kind,
  } = status;
  const alert = alerting({ status, skipped });

  let label = `v${current}`;
  if (installing) label = "Updating…";
  else if (checking) label = "Checking…";
  else if (available) label = `Update ${latest?.replace(/^v/, "")}`;

  const recheck = () => {
    setChecking(true);
    void check().finally(() => setChecking(false));
  };

  const copy = () => {
    if (!command) return;
    navigator.clipboard.writeText(command).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 2000);
        return true;
      },
      () => setCopied(false),
    );
  };

  // Ticking the box takes effect immediately, so closing by any route (button,
  // Escape, backdrop) keeps the choice rather than silently dropping it.
  const toggleSkip = (skip: boolean) => {
    setSkipThis(skip);
    if (latest) setSkipped(latest, skip);
  };
  const dismiss = () => setOpen(false);

  const skipBox = latest ? (
    <label className="modal-check">
      <input type="checkbox" checked={skipThis} onChange={(e) => toggleSkip(e.target.checked)} />
      Skip this version - don&apos;t alert me again until {latest} is superseded
    </label>
  ) : null;

  let title = `Up to date - checked ${sinceLabel(status.checked_at)}. Click to check now.`;
  if (available && !alert) title = `WaveSink ${latest} is available - skipped. Click to review.`;
  else if (available)
    title = `WaveSink ${latest} is available (checked ${sinceLabel(status.checked_at)})`;

  return (
    <>
      <button
        type="button"
        className={"hb-status hb-update" + (alert ? " avail" : "")}
        onClick={() => (available ? setOpen(true) : recheck())}
        disabled={installing || checking}
        title={status.error ? `Last check failed: ${status.error}` : title}
      >
        <span className="dot" />
        {label}
      </button>
      <Modal open={open} onClose={() => setOpen(false)} title={`WaveSink ${latest} is available`}>
        {can_self_update ? (
          <>
            <p className="modal-text">
              Downloads the new AppImage, verifies its checksum, and replaces{" "}
              <code>WaveSink.AppImage</code>. The running version keeps going until you restart.
            </p>
            {error && <p className="modal-text err">{error}</p>}
            {skipBox}
            <div className="modal-btns">
              <button
                type="button"
                className="modal-btn primary"
                disabled={installing}
                onClick={() => void install()}
              >
                {installing ? "Updating…" : `Update to ${latest}`}
              </button>
              <button type="button" className="modal-btn" onClick={dismiss}>
                Not now
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="modal-text">
              {kind === "nix"
                ? "This install is managed by Nix. Update it through your flake or profile."
                : can_launch_update
                  ? "Updating needs root, so this opens a terminal that asks for your password and runs:"
                  : command
                    ? "Updating a packaged install needs root, so WaveSink cannot do it itself. Run:"
                    : "WaveSink cannot identify how this copy was installed. Update it the same way you installed it."}
            </p>
            {command && <pre className="modal-code">{command}</pre>}
            {skipBox}
            {error && <p className="modal-text err">{error}</p>}
            <div className="modal-btns">
              {can_launch_update && (
                <button type="button" className="modal-btn primary" onClick={() => void launch()}>
                  Update to {latest}
                </button>
              )}
              {command && (
                <button
                  type="button"
                  className={"modal-btn" + (can_launch_update ? "" : " primary")}
                  onClick={copy}
                >
                  {copied ? "Copied" : "Copy command"}
                </button>
              )}
              {notes_url && (
                <a className="modal-btn" href={notes_url} target="_blank" rel="noreferrer">
                  Release notes
                </a>
              )}
              <button type="button" className="modal-btn" onClick={dismiss}>
                Close
              </button>
            </div>
          </>
        )}
      </Modal>
    </>
  );
}
