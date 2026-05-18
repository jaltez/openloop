import type { ProjectTask, SchedulerResult } from "../types.js";

export function shouldStopForNoProgress(input: {
  task: ProjectTask;
  previousFailureSignature: string | null;
  previousPromotionDecision: SchedulerResult["promotionDecision"] | null;
  currentPromotionDecision: SchedulerResult["promotionDecision"];
  beforeFingerprint: string | null;
  afterFingerprint: string | null;
  noProgressRepeatLimit: number;
}): boolean {
  const observations: string[] = [];
  const currentFailureSignature = input.task.lastFailureSignature;

  if (currentFailureSignature && input.previousFailureSignature === currentFailureSignature) {
    observations.push(`failure-${sanitizeObservationKey(currentFailureSignature)}`);
  }

  if (input.previousPromotionDecision === "blocked" && input.currentPromotionDecision === "blocked") {
    observations.push("promotion-blocked");
  }

  if (input.beforeFingerprint === input.afterFingerprint) {
    observations.push("diff-unchanged");
  }

  let blocked = false;
  for (const observation of observations) {
    const count = nextNoProgressCount(input.task.notes ?? [], observation);
    input.task.notes = [...(input.task.notes ?? []), `openloop:no-progress:${observation}:${count}`];
    if (count >= input.noProgressRepeatLimit) {
      blocked = true;
    }
  }

  return blocked;
}

function nextNoProgressCount(notes: string[], key: string): number {
  const prefix = `openloop:no-progress:${key}:`;
  for (let index = notes.length - 1; index >= 0; index -= 1) {
    const note = notes[index];
    if (!note?.startsWith(prefix)) {
      continue;
    }
    const count = Number(note.slice(prefix.length));
    return Number.isFinite(count) ? count + 1 : 1;
  }
  return 1;
}

function sanitizeObservationKey(value: string): string {
  return value.replace(/[^a-z0-9-]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();
}
