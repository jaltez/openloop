# OpenLoop Rust Port

Port 1:1 de la aplicación TypeScript a Rust, en `rust/`. Ambas versiones
coexisten en el repo: el TS original queda intacto en la raíz; el port vive
en `rust/` con su propio `Cargo.toml`.

## Build

```sh
cd rust
cargo build --release
# binario: rust/target/release/openloop (4.7 MB, estático salvo libc)
```

## Alcance del port

| Subsistema TS | Contraparte Rust | Estado |
|---|---|---|
| `src/core/*` (37 módulos) | `rust/src/core/*` | 1:1 |
| `src/daemon/worker.ts` | `rust/src/core/worker.rs` | 1:1 |
| `src/cli/commands/*` (17 comandos) | `rust/src/cli/*` (clap) | 1:1 |
| `src/tui/*` (OpenTUI/Solid) | `rust/src/tui/*` (ratatui) | aproximado |
| `templates/project/` | embebido en el binario (`include_dir`) | 1:1 |

Decisiones:

- **std puro, sin tokio**: el daemon es single-threaded con subprocesses, como
  el original. Timeout de agentes vía polling + `kill(-pgid)` con escalado
  SIGTERM→SIGKILL.
- **Formatos de estado idénticos**: mismos JSON/YAML (`config.json`,
  `projects.json`, `daemon-state.json`, `tasks.json`, promotions, approvals,
  events.jsonl). Un home `~/.openloop` puede alternar entre binarios TS y Rust.
- **8 proveedores de agente** (pi, claude, aider, codex, opencode, ka, omp,
  custom) con parsing de usage igual al TS (last-line JSON / objeto completo
  según proveedor; custom sin parsing).
- **TUI**: ratatui con 9 vistas (Dashboard, Projects, Tasks, Service, Config,
  Run, Promotions, Events, Logs), tab bar, status bar, help overlay (`?`),
  navegación 1-9/Tab, polling de estado por intervalos, edición inline.
- HTTP saliente (webhooks, GitHub/GitLab) vía `ureq`; dashboard web con
  servidor HTTP propio (solo GET, solo localhost).

## Métricas (medidas en este repo, 2026-10-01)

| Métrica | TS (node dist) | TS (bun dist) | Rust release | Ratio Rust/Node |
|---|---|---|---|---|
| `--help` wall | 76.8 ms | 182.2 ms | **1.2 ms** | ~64× más rápido |
| `status` wall | 78.7 ms | — | **1.1 ms** | ~72× |
| `events` wall | 79.1 ms | — | **1.1 ms** | ~72× |
| `doctor` wall | 270.9 ms | — | **~170 ms** | ~1.6× (dominado por subprocesses) |
| RSS `--help` | 79.3 MB | 95.0 MB | **3.4 MB** | ~23× menos |
| RSS daemon idle | ~71–75 MB | — | **5.0–5.2 MB** | ~14× menos |
| Artefacto desplegable | dist 756 KB + node_modules 258 MB + node 118 MB | ídem | **binario 4.7 MB** | ~80× menos en disco |
| Dependencia de runtime | Node ≥22 / Bun | ídem | ninguna (libc estándar) | |

Notas:

- El doctor de Rust sigue dominado por los subprocesses `which`/`--version`;
  la ventaja de arranque no aplica a comandos bound por I/O externo.
- El RSS del daemon Rust es estable (5.0→5.2 MB tras 16 s con ticks); el de
  Node baja de 74→71 MB tras el primer GC.
- La carga del sistema (CPU por tick) es despreciable en ambos; el daemon
  duerme entre ticks de 5 s.

## Verificación funcional (paridad)

Escenario idéntico ejecutado contra ambos binarios con homes separados
(`project add --init` → `task add` low-risk → provider custom → `run-once`
plan → commit → `run-once` implement → promotion list):

| Campo | TS | Rust |
|---|---|---|
| plan: mode/role/exitCode/taskStatus | plan / sdd-planner / 0 / ready | idéntico |
| implement: taskStatus/stoppedBy | blocked / no-progress | idéntico |
| implement: decision/action | manual-review / queue-review | idéntico |
| promotion queue | pending manual-review | idéntico |
| costUsd/costSource | 0.1 / estimated | idéntico |
| dirtyTreeDetected | false | idéntico |

También verificado en Rust: daemon lifecycle (start/tick/pause/resume/stop,
events.jsonl, daily digest), MCP stdio (initialize/tools/list/tools/call),
dashboard HTTP (HTML + /api/*), TUI en PTY (tab bar, vistas, help, quit),
cron, file-lock, plantillas embebidas materializadas + AGENTS.md merge.

## Limitaciones conocidas

- `setup` interactivo y `watch` requieren TTY real (igual que TS).
- Los mensajes de error/uso están alineados pero no son byte-idénticos
  (clap vs yargs).
- No se portaron los tests de vitest; la verificación es de humo
  comportamental comparada (arriba).
