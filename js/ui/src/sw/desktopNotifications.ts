export type DesktopNotificationIdentity = {
  connectionId: string;
  bootGeneration: string;
  notificationId: string;
  revision: string;
};

export function topLevelDesktopSender(
  source: { type?: string; frameType?: string } | null,
  preview: boolean,
): boolean {
  return (
    source?.type === "window" && source.frameType === "top-level" && !preview
  );
}

export function desktopNotificationIdentity(
  value: unknown,
): DesktopNotificationIdentity | null {
  const data = value as Partial<DesktopNotificationIdentity> | null;
  if (
    !data ||
    typeof data.connectionId !== "string" ||
    data.connectionId.length > 256 ||
    typeof data.bootGeneration !== "string" ||
    data.bootGeneration.length > 39 ||
    typeof data.notificationId !== "string" ||
    !/^[1-9]\d{0,19}$/.test(data.notificationId) ||
    typeof data.revision !== "string" ||
    !/^[1-9]\d{0,19}$/.test(data.revision)
  ) {
    return null;
  }
  return {
    connectionId: data.connectionId,
    bootGeneration: data.bootGeneration,
    notificationId: data.notificationId!,
    revision: data.revision!,
  };
}

export function desktopNotificationImage(value: unknown): string | undefined {
  return typeof value === "string" &&
    value.length <= 1_500_000 &&
    value.startsWith("data:image/png;base64,")
    ? value
    : undefined;
}

export function desktopNotificationSourceClientId(
  value: unknown,
): string | null {
  const sourceClientId = (value as { sourceClientId?: unknown } | null)
    ?.sourceClientId;
  return typeof sourceClientId === "string" &&
    sourceClientId.length > 0 &&
    sourceClientId.length <= 1_024
    ? sourceClientId
    : null;
}
