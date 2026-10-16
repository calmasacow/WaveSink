function sync(model, rows) {
  var ids = Object.create(null);
  for (var i = 0; i < rows.length; i++) {
    var row = rows[i];
    if (!row || typeof row.id !== "string" || ids[row.id]) throw new Error("Invalid WaveSink row");
    ids[row.id] = true;
    var found = -1;
    for (var j = i; j < model.count; j++) {
      if (model.get(j).rowId === row.id) {
        found = j;
        break;
      }
    }
    var item = {
      rowId: row.id,
      label: String(row.label),
      volume: Math.max(0, Math.min(100, row.volume)),
      muted: !!row.muted,
    };
    if (found < 0) model.insert(i, item);
    else {
      if (found !== i) model.move(found, i, 1);
      for (var key in item) model.setProperty(i, key, item[key]);
    }
  }
  while (model.count > rows.length) model.remove(model.count - 1);
}

// Same Standard and Pro scales as WaveSink.
function meter(peak, pro) {
  if (!pro) return Math.min(1, Math.sqrt(Math.max(0, peak)));
  if (!(peak > 0.001)) return 0;
  return Math.min(1, Math.max(0, ((20 * Math.log(peak)) / Math.LN10 + 60) / 60));
}

function palette(raw) {
  var colors = { green: "#22c55e", yellow: "#f59e0b", red: "#ef4444" };
  var lines = String(raw).split("\n");
  for (var i = 0; i < lines.length; i++) {
    var match = lines[i].match(
      /^\s*(green|yellow|red)\s*=\s*["']?(#[0-9a-fA-F]{6})["']?\s*(?:#.*)?$/,
    );
    if (match) colors[match[1]] = match[2];
  }
  return colors;
}

if (typeof module !== "undefined") module.exports = { sync: sync, meter: meter, palette: palette };
