import { describe, expect, it } from "vitest";
import { gainDb, meterPosition, peakOf, pushLevels, sendGain } from "./meters";

describe("meters", () => {
  it("reports the louder channel of a pushed level", () => {
    pushLevels({ sink_game: [0.5, 0.4] });
    expect(peakOf("sink_game")).toBe(0.5);
    expect(peakOf("sink_missing")).toBe(0);
    expect(peakOf(null)).toBe(0);
  });

  it("maps send percent onto the backend's cubic gain", () => {
    expect(sendGain(100)).toBe(1);
    expect(sendGain(0)).toBe(0);
    expect(sendGain(50)).toBeCloseTo(0.125);
  });

  it("places Standard levels by square root, as before", () => {
    expect(meterPosition(0.25, false)).toBeCloseTo(0.5);
    expect(meterPosition(1, false)).toBe(1);
  });

  it("places Pro levels linearly in dBFS from -60 to 0", () => {
    expect(meterPosition(1, true)).toBe(1);
    expect(meterPosition(10 ** (-18 / 20), true)).toBeCloseTo(0.7);
    expect(meterPosition(10 ** (-3 / 20), true)).toBeCloseTo(0.95);
    expect(meterPosition(10 ** (-80 / 20), true)).toBe(0);
    expect(meterPosition(0, true)).toBe(0);
  });

  it("labels slider gain in dB on the cubic volume curve", () => {
    expect(gainDb(100)).toBe("0.0 dB");
    expect(gainDb(50)).toBe("−18.1 dB");
    expect(gainDb(0)).toBe("−∞ dB");
  });
});
