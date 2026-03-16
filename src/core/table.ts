export type OutputFormat = "table" | "json";

export function resolveOutputFormat(flag: string | undefined): OutputFormat {
  if (flag === "table") return "table";
  if (flag === "json") return "json";
  return process.stdout.isTTY ? "table" : "json";
}

export function printTable(rows: Record<string, string | number | null | undefined>[]): void {
  if (rows.length === 0) {
    console.log("(empty)");
    return;
  }

  const keys = Object.keys(rows[0]!);
  const widths = keys.map((key) => {
    const maxVal = Math.max(...rows.map((row) => String(row[key] ?? "").length));
    return Math.max(key.length, maxVal);
  });

  const header = keys.map((key, i) => key.toUpperCase().padEnd(widths[i]!)).join("  ");
  const divider = widths.map((w) => "-".repeat(w)).join("  ");
  console.log(header);
  console.log(divider);
  for (const row of rows) {
    const line = keys.map((key, i) => String(row[key] ?? "").padEnd(widths[i]!)).join("  ");
    console.log(line);
  }
}
