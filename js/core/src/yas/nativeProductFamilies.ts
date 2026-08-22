/**
 * Native product clients used by the browser Workspace facade.
 *
 * This is a typed-family boundary rather than a transport adapter. Callers
 * retain the bigint handles and generations returned by the owning family.
 */

import {
  YAS_ENV_VERSION,
  YAS_EVENTS_VERSION,
  YAS_EXTENSION_VERSION,
  YAS_FAMILY_ENV,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_FS,
  YAS_FAMILY_GIT,
  YAS_FAMILY_KV,
  YAS_FAMILY_LSP,
  YAS_FAMILY_NET,
  YAS_FAMILY_PROCESS,
  YAS_FAMILY_CHANNEL,
  YAS_FS_VERSION,
  YAS_GIT_VERSION,
  YAS_KV_VERSION,
  YAS_LSP_VERSION,
  YAS_NET_VERSION,
  YAS_PROCESS_VERSION,
  YAS_CHANNEL_VERSION,
  YAS_STATUS_UNAVAILABLE,
  YAS_STATUS_UNSUPPORTED,
} from "./generated";
import { YasChannelClient } from "./channel";
import { YasEnvClient } from "./env";
import { YasEventsClient } from "./events";
import { YasExtensionClient } from "./extension";
import { YasFsClient } from "./fs";
import { YasGitClient } from "./git";
import { YasKvClient } from "./kv";
import { YasLspClient } from "./lsp";
import { YasNetClient } from "./net";
import { YasProcessClient } from "./process";
import type { YasConnection } from "./session";
import { YasProtocolError, YasResultError } from "./wire";

interface FamilyClientFactories {
  fs(connection: YasConnection): YasFsClient;
  git(connection: YasConnection): YasGitClient;
  lsp(connection: YasConnection): YasLspClient;
  kv(connection: YasConnection): YasKvClient;
  process(connection: YasConnection): YasProcessClient;
  net(connection: YasConnection): YasNetClient;
  channel(connection: YasConnection): YasChannelClient;
  extension(connection: YasConnection): YasExtensionClient;
  events(connection: YasConnection): YasEventsClient;
  env(connection: YasConnection): YasEnvClient;
}

const defaultFactories: FamilyClientFactories = {
  fs: (connection) => new YasFsClient(connection),
  git: (connection) => new YasGitClient(connection),
  lsp: (connection) => new YasLspClient(connection),
  kv: (connection) => new YasKvClient(connection),
  process: (connection) => new YasProcessClient(connection),
  net: (connection) => new YasNetClient(connection),
  channel: (connection) => new YasChannelClient(connection),
  extension: (connection) => new YasExtensionClient(connection),
  events: (connection) => new YasEventsClient(connection),
  env: (connection) => new YasEnvClient(connection),
};

type FamilyClientName = keyof FamilyClientFactories;

const familyIdentity: Record<
  FamilyClientName,
  readonly [family: number, version: number]
> = {
  fs: [YAS_FAMILY_FS, YAS_FS_VERSION],
  git: [YAS_FAMILY_GIT, YAS_GIT_VERSION],
  lsp: [YAS_FAMILY_LSP, YAS_LSP_VERSION],
  kv: [YAS_FAMILY_KV, YAS_KV_VERSION],
  process: [YAS_FAMILY_PROCESS, YAS_PROCESS_VERSION],
  net: [YAS_FAMILY_NET, YAS_NET_VERSION],
  channel: [YAS_FAMILY_CHANNEL, YAS_CHANNEL_VERSION],
  extension: [YAS_FAMILY_EXTENSION, YAS_EXTENSION_VERSION],
  events: [YAS_FAMILY_EVENTS, YAS_EVENTS_VERSION],
  env: [YAS_FAMILY_ENV, YAS_ENV_VERSION],
};

type FamilyClient<K extends FamilyClientName> = ReturnType<
  FamilyClientFactories[K]
>;

/** Direct typed clients for every non-presentation family used by Workspace. */
export class YasNativeProductFamilies {
  private readonly factories: FamilyClientFactories;
  private readonly clients = new Map<FamilyClientName, unknown>();
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    factories: Partial<FamilyClientFactories> = {},
  ) {
    this.factories = { ...defaultFactories, ...factories };
  }

  get fs(): YasFsClient | null {
    return this.optional("fs");
  }

  get git(): YasGitClient | null {
    return this.optional("git");
  }

  get lsp(): YasLspClient | null {
    return this.optional("lsp");
  }

  get kv(): YasKvClient | null {
    return this.optional("kv");
  }

  get process(): YasProcessClient | null {
    return this.optional("process");
  }

  /** Existing Workspace consumers already use this native typed getter. */
  get processProtocol(): YasProcessClient | null {
    return this.process;
  }

  get net(): YasNetClient | null {
    return this.optional("net");
  }

  get channel(): YasChannelClient | null {
    return this.optional("channel");
  }

  get extension(): YasExtensionClient | null {
    return this.optional("extension");
  }

  get events(): YasEventsClient | null {
    return this.optional("events");
  }

  /** Existing Workspace consumers already use this native typed getter. */
  get eventsProtocol(): YasEventsClient | null {
    return this.events;
  }

  get env(): YasEnvClient | null {
    return this.optional("env");
  }

  /** Existing Workspace consumers already use this native typed getter. */
  get envProtocol(): YasEnvClient | null {
    return this.env;
  }

  // High-level operations mirror native family ownership boundaries; every
  // returned resource keeps its server-issued bigint handle.

  openFs(
    ...args: Parameters<YasFsClient["open"]>
  ): ReturnType<YasFsClient["open"]> {
    return this.require("fs").open(...args);
  }

  openGit(
    ...args: Parameters<YasGitClient["open"]>
  ): ReturnType<YasGitClient["open"]> {
    return this.require("git").open(...args);
  }

  discoverGit(
    ...args: Parameters<YasGitClient["discover"]>
  ): ReturnType<YasGitClient["discover"]> {
    return this.require("git").discover(...args);
  }

  openLsp(
    ...args: Parameters<YasLspClient["open"]>
  ): ReturnType<YasLspClient["open"]> {
    return this.require("lsp").open(...args);
  }

  listLspServers(
    ...args: Parameters<YasLspClient["listServers"]>
  ): ReturnType<YasLspClient["listServers"]> {
    return this.require("lsp").listServers(...args);
  }

  stopLspServer(
    ...args: Parameters<YasLspClient["stopServer"]>
  ): ReturnType<YasLspClient["stopServer"]> {
    return this.require("lsp").stopServer(...args);
  }

  openKv(
    ...args: Parameters<YasKvClient["open"]>
  ): ReturnType<YasKvClient["open"]> {
    return this.require("kv").open(...args);
  }

  listProcesses(
    ...args: Parameters<YasProcessClient["list"]>
  ): ReturnType<YasProcessClient["list"]> {
    return this.require("process").list(...args);
  }

  spawnProcess(
    ...args: Parameters<YasProcessClient["spawn"]>
  ): ReturnType<YasProcessClient["spawn"]> {
    return this.require("process").spawn(...args);
  }

  attachProcess(
    ...args: Parameters<YasProcessClient["attach"]>
  ): ReturnType<YasProcessClient["attach"]> {
    return this.require("process").attach(...args);
  }

  controlProcess(
    ...args: Parameters<YasProcessClient["control"]>
  ): ReturnType<YasProcessClient["control"]> {
    return this.require("process").control(...args);
  }

  waitProcess(
    ...args: Parameters<YasProcessClient["wait"]>
  ): ReturnType<YasProcessClient["wait"]> {
    return this.require("process").wait(...args);
  }

  openNet(
    ...args: Parameters<YasNetClient["open"]>
  ): ReturnType<YasNetClient["open"]> {
    return this.require("net").open(...args);
  }

  listChannels(
    ...args: Parameters<YasChannelClient["catalogue"]["firstSnapshot"]>
  ): ReturnType<YasChannelClient["catalogue"]["firstSnapshot"]> {
    return this.require("channel").catalogue.firstSnapshot(...args);
  }

  listenChannel(
    ...args: Parameters<YasChannelClient["listen"]>
  ): ReturnType<YasChannelClient["listen"]> {
    return this.require("channel").listen(...args);
  }

  connectChannel(
    ...args: Parameters<YasChannelClient["connect"]>
  ): ReturnType<YasChannelClient["connect"]> {
    return this.require("channel").connect(...args);
  }

  listExtensions(
    ...args: Parameters<YasExtensionClient["list"]>
  ): ReturnType<YasExtensionClient["list"]> {
    return this.require("extension").list(...args);
  }

  beginExtensionObject(
    ...args: Parameters<YasExtensionClient["beginObject"]>
  ): ReturnType<YasExtensionClient["beginObject"]> {
    return this.require("extension").beginObject(...args);
  }

  uploadExtensionObject(
    ...args: Parameters<YasExtensionClient["uploadObject"]>
  ): ReturnType<YasExtensionClient["uploadObject"]> {
    return this.require("extension").uploadObject(...args);
  }

  commitExtensionObject(
    ...args: Parameters<YasExtensionClient["commitObject"]>
  ): ReturnType<YasExtensionClient["commitObject"]> {
    return this.require("extension").commitObject(...args);
  }

  deployExtension(
    ...args: Parameters<YasExtensionClient["deploy"]>
  ): ReturnType<YasExtensionClient["deploy"]> {
    return this.require("extension").deploy(...args);
  }

  controlExtension(
    ...args: Parameters<YasExtensionClient["control"]>
  ): ReturnType<YasExtensionClient["control"]> {
    return this.require("extension").control(...args);
  }

  followExtension(
    ...args: Parameters<YasExtensionClient["follow"]>
  ): ReturnType<YasExtensionClient["follow"]> {
    return this.require("extension").follow(...args);
  }

  discoverExtensionCommands(
    ...args: Parameters<YasExtensionClient["discoverCommands"]>
  ): ReturnType<YasExtensionClient["discoverCommands"]> {
    return this.require("extension").discoverCommands(...args);
  }

  onExtensionAttempt(
    ...args: Parameters<YasExtensionClient["onAttemptContext"]>
  ): ReturnType<YasExtensionClient["onAttemptContext"]> {
    return this.require("extension").onAttemptContext(...args);
  }

  getEventsConfig(
    ...args: Parameters<YasEventsClient["getConfig"]>
  ): ReturnType<YasEventsClient["getConfig"]> {
    return this.require("events").getConfig(...args);
  }

  setEventsConfig(
    ...args: Parameters<YasEventsClient["setConfig"]>
  ): ReturnType<YasEventsClient["setConfig"]> {
    return this.require("events").setConfig(...args);
  }

  dumpEvents(
    ...args: Parameters<YasEventsClient["dump"]>
  ): ReturnType<YasEventsClient["dump"]> {
    return this.require("events").dump(...args);
  }

  startEventsStream(
    ...args: Parameters<YasEventsClient["startStream"]>
  ): ReturnType<YasEventsClient["startStream"]> {
    return this.require("events").startStream(...args);
  }

  stopEventsStream(
    ...args: Parameters<YasEventsClient["stopStream"]>
  ): ReturnType<YasEventsClient["stopStream"]> {
    return this.require("events").stopStream(...args);
  }

  startEventsRecording(
    ...args: Parameters<YasEventsClient["startRecording"]>
  ): ReturnType<YasEventsClient["startRecording"]> {
    return this.require("events").startRecording(...args);
  }

  stopEventsRecording(
    ...args: Parameters<YasEventsClient["stopRecording"]>
  ): ReturnType<YasEventsClient["stopRecording"]> {
    return this.require("events").stopRecording(...args);
  }

  listEventsRecordings(
    ...args: Parameters<YasEventsClient["listRecordings"]>
  ): ReturnType<YasEventsClient["listRecordings"]> {
    return this.require("events").listRecordings(...args);
  }

  getEnvironment(
    ...args: Parameters<YasEnvClient["get"]>
  ): ReturnType<YasEnvClient["get"]> {
    return this.require("env").get(...args);
  }

  /** True only while the exact family version is selected and available. */
  supports(name: FamilyClientName): boolean {
    if (this.disposed) return false;
    const [family, version] = familyIdentity[name];
    try {
      this.connection.family(family, version);
      return true;
    } catch (error) {
      if (
        error instanceof YasResultError &&
        (error.status === YAS_STATUS_UNSUPPORTED ||
          error.status === YAS_STATUS_UNAVAILABLE)
      )
        return false;
      throw error;
    }
  }

  /**
   * Require one negotiated family. The returned client speaks only that YAS
   * family (and its declared Transfer/State dependencies).
   */
  require<K extends FamilyClientName>(name: K): FamilyClient<K> {
    if (this.disposed)
      throw new YasProtocolError("native product families are disposed");
    if (!this.supports(name)) {
      const [family, version] = familyIdentity[name];
      throw new YasResultError(
        YAS_STATUS_UNSUPPORTED,
        new Uint8Array(),
        `YAS family 0x${family.toString(16)} version ${version} is unavailable`,
      );
    }
    const existing = this.clients.get(name);
    if (existing) return existing as FamilyClient<K>;
    const created = this.factories[name](this.connection) as FamilyClient<K>;
    this.clients.set(name, created);
    return created;
  }

  /** Release clients that own event listeners or active remote resources. */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const client of this.clients.values()) {
      const disposable = client as { dispose?: () => void };
      try {
        disposable.dispose?.();
      } catch {
        // One broken cleanup must not retain the remaining family clients.
      }
    }
    this.clients.clear();
  }

  private optional<K extends FamilyClientName>(
    name: K,
  ): FamilyClient<K> | null {
    return this.supports(name) ? this.require(name) : null;
  }
}
