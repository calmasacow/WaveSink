const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const context = { module: { exports: {} } };
vm.runInNewContext(fs.readFileSync(__dirname + "/Model.js", "utf8"), context);
const { sync, meter, palette } = context.module.exports;
const rows = [];
const model = {
  get count() {
    return rows.length;
  },
  get(i) {
    return rows[i];
  },
  insert(i, row) {
    rows.splice(i, 0, row);
  },
  move(from, to) {
    rows.splice(to, 0, rows.splice(from, 1)[0]);
  },
  setProperty(i, key, value) {
    rows[i][key] = value;
  },
  remove(i) {
    rows.splice(i, 1);
  },
};
const make = (id, label) => ({ id, label, volume: 73, muted: false });
sync(model, [make("custom-A", "Podcast"), make("custom-B", "Guests")]);
const retained = rows[1];
sync(model, [make("custom-B", "Renamed"), make("custom-C", "Live")]);
assert.equal(rows[0], retained, "rename/reorder recreated existing delegate");
assert.equal(rows[0].label, "Renamed");
assert.equal(rows[1].rowId, "custom-C");
sync(model, []);
assert.equal(rows.length, 0);
assert.equal(meter(0), 0);
assert.equal(meter(0.001, true), 0);
assert.equal(meter(1), 1);
assert.ok(Math.abs(meter(0.1, true) - 2 / 3) < 1e-10);
assert.equal(meter(0.25, false), 0.5);
assert.equal(meter(4, false), 1);
assert.equal(meter(-1, false), 0);
const colors = palette('green = "#123456"\nyellow = "#abcdef"\nred = "#fedcba"');
assert.equal(colors.green, "#123456");
assert.equal(colors.yellow, "#abcdef");
assert.equal(colors.red, "#fedcba");
assert.equal(palette('red = "invalid"').red, "#ef4444");
console.log("Panel models: dynamic IDs, rename, reorder, removal, meter scale passed");
