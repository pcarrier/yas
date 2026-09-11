import { createRequire } from "node:module";
import type { Sandbox } from "tensorlake";

export const imageName = "yas-ubuntu-26-04";

export function apiKey(): string {
  const key = process.env.TENSORLAKE_API_KEY;
  if (!key?.trim())
    throw new Error("Set TENSORLAKE_API_KEY before provisioning.");
  return key;
}

export async function startupFailure(
  sandbox: Pick<Sandbox, "sandboxId" | "info">,
  cause: unknown,
): Promise<Error> {
  const lines = [`Sandbox ${sandbox.sandboxId} failed startup.`];
  try {
    // A proxy 404 can mean the sandbox died after create returned Running.
    // Read the control plane to report the actual termination reason.
    const info = await sandbox.info();
    lines.push(
      JSON.stringify(
        {
          status: info.status,
          terminationReason: info.terminationReason,
          errorDetails: info.errorDetails,
          outcome: info.outcome,
          traceId: info.traceId,
        },
        null,
        2,
      ),
    );
  } catch (error) {
    lines.push(
      `Could not read sandbox state: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  lines.push(cause instanceof Error ? cause.message : String(cause));
  return new Error(lines.join("\n"), { cause });
}

// tensorlake 0.5.132's public TS image wrapper drops `cas`. Use its shipped
// Rust SDK binding until the public wrapper exposes the flag. Keep the SDK
// pinned and validate this adapter when upgrading it.
export async function buildCasImage(
  dockerfilePath: string,
  registeredName: string,
) {
  const require = createRequire(import.meta.url);
  const sdkRequire = createRequire(require.resolve("tensorlake"));
  let target = `${process.platform}-${process.arch}`;
  if (process.platform === "linux") {
    const report = process.report.getReport() as {
      header: { glibcVersionRuntime?: string };
    };
    target += report.header.glibcVersionRuntime ? "-gnu" : "-musl";
  }
  const binding = sdkRequire(`tensorlake-native-${target}`) as {
    buildSandboxImage(
      options: Record<string, unknown>,
      emit: (event: { message: string }) => void,
    ): Promise<string>;
  };
  const result: unknown = JSON.parse(
    await binding.buildSandboxImage(
      {
        apiUrl: process.env.TENSORLAKE_API_URL || "https://api.tensorlake.ai",
        bearerToken: apiKey(),
        organizationId: process.env.TENSORLAKE_ORGANIZATION_ID,
        projectId: process.env.TENSORLAKE_PROJECT_ID,
        namespace: process.env.INDEXIFY_NAMESPACE || "default",
        dockerfilePath,
        registeredName,
        cas: true,
        isPublic: false,
        cpus: 8,
        memoryMb: 16 * 1024,
        diskMb: 32 * 1024,
      },
      (event) => {
        // Never throw across the native SDK callback boundary.
        try {
          process.stderr.write(`${event.message}\n`);
        } catch {
          /* closed stderr */
        }
      },
    ),
  );
  // A legacy rootfs image is unusable with GPUs. Never silently accept one.
  if (
    !result ||
    typeof result !== "object" ||
    !("image_id" in result) ||
    typeof result.image_id !== "string" ||
    !/^[a-f0-9]{64}$/.test(result.image_id)
  ) {
    throw new Error(
      "Tensorlake did not return a CAS image_id; check SDK CAS support.",
    );
  }
  return {
    image: `cas-v1:${result.image_id}`,
    registeredName,
    metadata: result,
  };
}
