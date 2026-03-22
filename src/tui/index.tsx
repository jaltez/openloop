import { render } from "@opentui/solid";
import { createCliRenderer } from "@opentui/core";
import App from "./App.js";

export async function launchTUI(version: string): Promise<void> {
  const renderer = await createCliRenderer({
    exitOnCtrlC: true,
    useAlternateScreen: true,
    targetFps: 30,
  });

  render(() => <App version={version} />, renderer);

  // Keep process alive until renderer is destroyed
  await new Promise<void>((resolve) => {
    renderer.on("destroy", resolve);
  });
}
