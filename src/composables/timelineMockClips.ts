import type { ProjectTimelineClip } from "./timelineFrame";
import { TIMELINE_CHANNEL_LABELS } from "./timelineFrame";

/** Включить тестовые клипы вместо IPC (для отладки отрисовки). */
export const USE_MOCK_TIMELINE_CLIPS = true;

type MockSpec = {
  channel: string;
  startOffsetMin: number;
  durationMin: number;
  label: string;
};

const MOCK_SPECS: MockSpec[] = [
  { channel: "logs", startOffsetMin: -90, durationMin: 35, label: "output mock A" },
  { channel: "logs", startOffsetMin: -40, durationMin: 18, label: "output mock B" },
  { channel: "logs", startOffsetMin: -12, durationMin: 8, label: "output mock C" },
  { channel: "logs", startOffsetMin: -3 * 24 * 60, durationMin: 2 * 24 * 60, label: "output mock long" },
  { channel: "trigger", startOffsetMin: -75, durationMin: 22, label: "trigger mock A" },
  { channel: "trigger", startOffsetMin: -25, durationMin: 12, label: "trigger mock B" },
  { channel: "spectrogram", startOffsetMin: -55, durationMin: 28, label: "knock mock A" },
  { channel: "spectrogram", startOffsetMin: -8, durationMin: 6, label: "knock mock B" },
  { channel: "runs", startOffsetMin: -120, durationMin: 45, label: "run mock A" },
  { channel: "runs", startOffsetMin: -60, durationMin: 20, label: "run mock B" },
  { channel: "runs", startOffsetMin: -5, durationMin: 4, label: "run mock C" },
];

function validChannel(id: string): boolean {
  return TIMELINE_CHANNEL_LABELS.some((ch) => ch.id === id);
}

/** Клипы вокруг Date.now() — видны при span ≈ 1 ч. */
export function buildMockTimelineClips(nowMs = Date.now()): ProjectTimelineClip[] {
  return MOCK_SPECS.map((spec, idx) => {
    const channel = validChannel(spec.channel) ? spec.channel : "logs";
    const startMs = nowMs + spec.startOffsetMin * 60_000;
    const endMs = startMs + spec.durationMin * 60_000;
    return {
      id: `mock-${channel}-${idx}`,
      channel,
      startMs,
      endMs,
      record: { path: `mock/${channel}/${idx}.csv`, kind: channel },
      label: spec.label,
    };
  });
}
