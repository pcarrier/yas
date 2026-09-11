#!/usr/bin/env node
import { parseArgs } from "node:util";
import { Sandbox } from "tensorlake";
import { apiKey, imageName, startupFailure } from "./tensorlake/common.ts";
import { launchYas } from "./tensorlake/launch.ts";
import { sandboxTimeout, shareConfiguration } from "./tensorlake/startup.ts";

const { values } = parseArgs({
  options: {
    image: { type: "string" },
    name: { type: "string" },
    sandbox: { type: "string" },
    passphrase: { type: "string" },
    timeout: { type: "string" },
    help: { type: "boolean", short: "h" },
  },
});
if (values.help) {
  console.log(
    "Usage: bin/tensorlake-start.ts [--sandbox ID] [--image NAME_OR_CAS_REF] [--name NAME] [--timeout SECONDS] [--passphrase PASSPHRASE]\nStart YAS server --share --edge with 8 CPUs, 16 GiB RAM and one RTX-PRO-6000; print its HTTPS edge and share URLs. Default timeout: 0 (plan maximum; unlimited where supported).\nPassphrase defaults to YAS_SHARE_PASSPHRASE, a saved value, or a fresh random value.\n--sandbox reuses an existing sandbox without changing its resources or timeout.",
  );
} else {
  if (values.sandbox && [values.image, values.name, values.timeout].some((value) => value !== undefined)) {
    throw new Error("--sandbox cannot be combined with --image, --name, or --timeout.");
  }
  const timeoutSecs = sandboxTimeout(values.timeout);
  const configuredPassphrase = values.passphrase ?? process.env.YAS_SHARE_PASSPHRASE;
  if (configuredPassphrase !== undefined) shareConfiguration(configuredPassphrase);
  const sandbox = values.sandbox
    ? await Sandbox.connect({ sandboxId: values.sandbox, apiKey: apiKey() })
    : await Sandbox.create({
      apiKey: apiKey(),
      image: values.image ?? imageName,
      name: values.name,
      cpus: 8,
      memoryMb: 16 * 1024,
      // Tensorlake owns PID 1; image entrypoints run as ordinary child processes.
      // Override older images that tried to boot systemd.
      entrypoint: ["/bin/sleep", "infinity"],
      gpu: { count: 1, model: "RTX-PRO-6000" },
      timeoutSecs,
    });
  console.error(`Sandbox ${sandbox.sandboxId} ${values.sandbox ? "connected" : "created"}.`);
  try {
    const result = await launchYas(sandbox, configuredPassphrase);
    const info = await sandbox.info();
    console.error(result.gpu);
    console.error(result.extensions);
    console.log(
      JSON.stringify(
        {
          sandboxId: sandbox.sandboxId,
          image: info.image,
          timeoutSecs: info.timeoutSecs,
          edgeUrl: result.edgeUrl,
          shareUrl: result.shareUrl,
        },
        null,
        2,
      ),
    );
  } catch (error) {
    throw await startupFailure(sandbox, error);
  } finally {
    sandbox.close(); // Release local SDK handles; keep the sandbox running.
  }
}
