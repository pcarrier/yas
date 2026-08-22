import type { YasContext, YasHost } from "../../typescript/yas";

export type CheckStatus = "pass" | "warn" | "fail";

export interface DoctorCheck {
  readonly key: string;
  readonly status: CheckStatus;
  readonly label: string;
  readonly detail: string;
}

export interface DoctorCapability {
  readonly key: string;
  readonly label: string;
  readonly familyId: number;
  readonly available: boolean;
}

export interface DoctorReport {
  readonly schema: "yas.doctor.v1";
  readonly status: "healthy" | "degraded";
  readonly server: {
    readonly protocolMinor: number;
    readonly release: string;
    readonly name: string;
    readonly bootId: string;
    readonly sessionId: string;
    readonly families: readonly number[];
    readonly realtimeUnixNanos: string | null;
  };
  readonly extension: {
    readonly runtime: "quickjs";
    readonly name: string | null;
    readonly id: string;
    readonly revision: string;
    readonly attempt: string;
    readonly taskId: number;
    readonly contentHash: string;
    readonly detached: boolean;
    readonly persistent: boolean;
    readonly enabled: boolean;
    readonly desiredRunning: boolean;
  };
  readonly checks: readonly DoctorCheck[];
  readonly capabilities: readonly DoctorCapability[];
  readonly summary: {
    readonly passed: number;
    readonly warnings: number;
    readonly failed: number;
    readonly availableCapabilities: number;
    readonly unavailableCapabilities: number;
  };
}

interface CapabilityDefinition {
  readonly key: string;
  readonly label: string;
  readonly familyId: number;
}

const CAPABILITIES: readonly CapabilityDefinition[] = [
  { key: "terminal", label: "terminal", familyId: 16 },
  { key: "surface", label: "surface", familyId: 32 },
  { key: "desktop", label: "desktop", familyId: 34 },
  { key: "media", label: "media", familyId: 35 },
  { key: "files", label: "files", familyId: 48 },
  { key: "git", label: "git", familyId: 49 },
  { key: "lsp", label: "LSP", familyId: 50 },
  { key: "kv", label: "KV", familyId: 51 },
  { key: "process", label: "process", familyId: 64 },
  { key: "network", label: "network", familyId: 65 },
  { key: "channel", label: "channel", familyId: 66 },
  { key: "extension", label: "extension", familyId: 67 },
  { key: "environment", label: "environment", familyId: 69 },
];

function check(
  checks: DoctorCheck[],
  key: string,
  status: CheckStatus,
  label: string,
  detail: string,
): void {
  checks.push({ key, status, label, detail });
}

function extensionLabel(context: YasContext): string {
  return typeof context.name === "string" && context.name.length > 0
    ? `@${context.name}`
    : "unnamed extension";
}

export function inspectDoctor(
  host: Pick<
    YasHost,
    "context" | "monotonicNow" | "random" | "realtimeNow" | "sleep"
  >,
): DoctorReport {
  const context = host.context;
  const checks: DoctorCheck[] = [];
  let realtimeUnixNanos: bigint | null = null;

  check(
    checks,
    "protocol",
    context.protocolMinor === 1 ? "pass" : "fail",
    "protocol",
    context.protocolMinor === 1
      ? "YAS v1 minor 1"
      : `YAS v1 minor ${context.protocolMinor}; this extension expects minor 1`,
  );

  const hasCommandFamilies =
    context.families.includes(66) && context.families.includes(67);
  check(
    checks,
    "command_transport",
    hasCommandFamilies ? "pass" : "fail",
    "command transport",
    "yas.cli.v1 invocation arrived over a native channel",
  );

  const named = typeof context.name === "string" && context.name.length > 0;
  const identityHealthy =
    named &&
    context.extensionHandle > 0n &&
    context.definitionRevision > 0n &&
    context.attempt > 0n &&
    /^[0-9a-f]{64}$/.test(context.contentHash);
  check(
    checks,
    "identity",
    identityHealthy ? "pass" : "fail",
    "extension identity",
    identityHealthy
      ? `${extensionLabel(context)} revision ${context.definitionRevision}, attempt ${context.attempt}`
      : "name, IDs, or module digest are invalid",
  );

  const lifecycleHealthy =
    context.persistent && context.enabled && context.desiredRunning;
  check(
    checks,
    "lifecycle",
    lifecycleHealthy ? "pass" : "fail",
    "lifecycle",
    lifecycleHealthy
      ? "persistent, enabled, and desired-running"
      : `persistent=${context.persistent}, enabled=${context.enabled}, desired-running=${context.desiredRunning}`,
  );

  const hasServerIdentity =
    context.serverRelease.length > 0 && /^[0-9a-f]{32}$/.test(context.bootId);
  check(
    checks,
    "server_identity",
    hasServerIdentity ? "pass" : "warn",
    "server identity",
    hasServerIdentity
      ? `${context.serverName} ${context.serverRelease}, boot ${context.bootId}`
      : "server release or boot ID is invalid",
  );

  try {
    const before = host.monotonicNow();
    realtimeUnixNanos = host.realtimeNow();
    host.sleep(1);
    const after = host.monotonicNow();
    const delta = after - before;
    const healthy = delta >= 0n && realtimeUnixNanos > 0n;
    check(
      checks,
      "clocks",
      healthy ? "pass" : "fail",
      "clocks",
      healthy
        ? `${(Number(delta) / 1_000_000).toFixed(3)} ms monotonic sleep; realtime available`
        : "monotonic or realtime clock returned an invalid value",
    );
  } catch (error) {
    check(checks, "clocks", "fail", "clocks", String(error));
  }

  try {
    const random = host.random(32);
    check(
      checks,
      "entropy",
      random.length === 32 ? "pass" : "fail",
      "entropy",
      `${random.length} of 32 requested bytes returned`,
    );
  } catch (error) {
    check(checks, "entropy", "fail", "entropy", String(error));
  }

  const capabilities = CAPABILITIES.map((definition) => ({
    ...definition,
    available: context.families.includes(definition.familyId),
  }));
  const passed = checks.filter((item) => item.status === "pass").length;
  const warnings = checks.filter((item) => item.status === "warn").length;
  const failed = checks.filter((item) => item.status === "fail").length;
  const availableCapabilities = capabilities.filter(
    (item) => item.available,
  ).length;

  return {
    schema: "yas.doctor.v1",
    status: failed === 0 ? "healthy" : "degraded",
    server: {
      protocolMinor: context.protocolMinor,
      release: context.serverRelease,
      name: context.serverName,
      bootId: context.bootId,
      sessionId: context.sessionId,
      families: [...context.families],
      realtimeUnixNanos: realtimeUnixNanos?.toString() ?? null,
    },
    extension: {
      runtime: "quickjs",
      name: named ? context.name! : null,
      id: context.extensionHandle.toString(),
      revision: context.definitionRevision.toString(),
      attempt: context.attempt.toString(),
      taskId: context.taskId,
      contentHash: context.contentHash,
      detached: context.detached,
      persistent: context.persistent,
      enabled: context.enabled,
      desiredRunning: context.desiredRunning,
    },
    checks,
    capabilities,
    summary: {
      passed,
      warnings,
      failed,
      availableCapabilities,
      unavailableCapabilities: capabilities.length - availableCapabilities,
    },
  };
}

function capabilityLines(report: DoctorReport): string[] {
  const capabilities = report.capabilities;
  const available = capabilities
    .filter((item) => item.available)
    .map((item) => item.label);
  const unavailable = capabilities
    .filter((item) => !item.available)
    .map((item) => item.label);
  return [
    ...wrapList("  ✓ available: ", available),
    ...wrapList("  ○ absent:    ", unavailable),
  ];
}

function wrapList(prefix: string, values: readonly string[]): string[] {
  if (values.length === 0) return [`${prefix}none`];
  const continuation = " ".repeat(prefix.length);
  const lines: string[] = [];
  let line = prefix;
  for (const value of values) {
    const separator = line === prefix ? "" : ", ";
    if (
      line.length > prefix.length &&
      line.length + separator.length + value.length > 88
    ) {
      lines.push(`${line},`);
      line = continuation + value;
    } else {
      line += separator + value;
    }
  }
  lines.push(line);
  return lines;
}

export function renderDoctor(report: DoctorReport): string {
  const extensionId = BigInt(report.extension.id)
    .toString(16)
    .padStart(16, "0");
  const checkLines = report.checks.map((item) => {
    const symbol =
      item.status === "pass" ? "✓" : item.status === "warn" ? "!" : "✗";
    return `  ${symbol} ${item.label}: ${item.detail}`;
  });

  return [
    "YAS doctor",
    "",
    "Server",
    `  YAS v1 minor ${report.server.protocolMinor} · ${report.server.name} ${report.server.release} · boot ${report.server.bootId}`,
    `  session ${report.server.sessionId}`,
    "",
    "Extension",
    `  @${report.extension.name ?? "unnamed"} · id ${extensionId} · revision ${report.extension.revision} · attempt ${report.extension.attempt}`,
    `  native QuickJS · task ${report.extension.taskId} · object ${report.extension.contentHash.slice(0, 12)}…`,
    "",
    "Checks",
    ...checkLines,
    "",
    "Native families",
    ...capabilityLines(report),
    "",
    "Summary",
    `  ${report.status} — ${report.summary.passed} passed, ${report.summary.warnings} warnings, ${report.summary.failed} failed; ${report.summary.unavailableCapabilities} optional capabilities absent`,
    "",
  ].join("\n");
}
