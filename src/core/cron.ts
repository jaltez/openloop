// Minimal 5-field cron matching (minute hour day-of-month month day-of-week),
// evaluated in UTC. Supports "*", "*/n", "a-b" ranges, and comma lists.

interface CronField {
  min: number;
  max: number;
  values: ReadonlySet<number>;
}

const FIELD_RANGES: Array<[number, number]> = [
  [0, 59], // minute
  [0, 23], // hour
  [1, 31], // day of month
  [1, 12], // month
  [0, 6],  // day of week (0 = Sunday)
];

function parseField(expr: string, fieldIndex: number, fullExpr: string): CronField {
  const [min, max] = FIELD_RANGES[fieldIndex]!;
  const values = new Set<number>();

  for (const part of expr.split(",")) {
    if (part === "") {
      throw new Error(`Invalid cron expression: ${fullExpr}`);
    }
    const [body, stepPart] = part.split("/");
    let step = 1;
    if (stepPart !== undefined) {
      if (!/^\d+$/.test(stepPart)) {
        throw new Error(`Invalid cron expression: ${fullExpr}`);
      }
      step = Number.parseInt(stepPart, 10);
      if (step < 1) {
        throw new Error(`Invalid cron expression: ${fullExpr}`);
      }
    }

    let from = min;
    let to = max;
    if (body !== "*") {
      const range = body.split("-");
      if (range.length === 1) {
        if (!/^\d+$/.test(range[0]!)) {
          throw new Error(`Invalid cron expression: ${fullExpr}`);
        }
        from = Number.parseInt(range[0]!, 10);
        to = stepPart !== undefined ? max : from;
      } else if (range.length === 2) {
        if (!/^\d+$/.test(range[0]!) || !/^\d+$/.test(range[1]!)) {
          throw new Error(`Invalid cron expression: ${fullExpr}`);
        }
        from = Number.parseInt(range[0]!, 10);
        to = Number.parseInt(range[1]!, 10);
      } else {
        throw new Error(`Invalid cron expression: ${fullExpr}`);
      }
    }

    if (from < min || to > max || from > to) {
      throw new Error(`Invalid cron expression: ${fullExpr}`);
    }

    for (let value = from; value <= to; value += step) {
      values.add(value);
    }
  }

  return { min, max, values };
}

/**
 * Whether `date` (evaluated in UTC) matches the given 5-field cron expression.
 * Throws on malformed expressions.
 */
export function matchesFiveFieldCron(expr: string, date: Date): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) {
    throw new Error(`Invalid cron expression: ${expr}`);
  }

  const minute = parseField(fields[0]!, 0, expr);
  const hour = parseField(fields[1]!, 1, expr);
  const dom = parseField(fields[2]!, 2, expr);
  const month = parseField(fields[3]!, 3, expr);
  const dow = parseField(fields[4]!, 4, expr);
  if (!month.values.has(date.getUTCMonth() + 1)) return false;

  if (!minute.values.has(date.getUTCMinutes())) return false;
  if (!hour.values.has(date.getUTCHours())) return false;
  // Standard cron: when both dom and dow are restricted, either may match.
  const domRestricted = fields[2] !== "*";
  const dowRestricted = fields[4] !== "*";
  const domMatch = dom.values.has(date.getUTCDate());
  const dowMatch = dow.values.has(date.getUTCDay());
  if (domRestricted && dowRestricted) {
    return domMatch || dowMatch;
  }
  return domMatch && dowMatch;
}
