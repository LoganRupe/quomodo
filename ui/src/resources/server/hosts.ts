import { useRead, useServerView } from "@/lib/hooks";
import { Types } from "komodo_client";
import { serverStateIntention } from "@/lib/color";
import { useMemo } from "react";

/** The agents running on one machine. */
export interface ServerHost {
  /** The hostname the agents report, or the Server name
   * of an agent that never reported one. */
  name: string;
  /** The root agent first, then rootless agents by user. */
  servers: Types.ServerListItem[];
  /** The agent the host reads its stats from: the root agent
   * when it has stats, otherwise any agent that does.
   * Agents on one machine all report the same host stats. */
  primary: Types.ServerListItem;
  /** NotOk if any agent is, otherwise Ok if any agent is. */
  state: Types.ServerState;
  /** True when the agents run different versions. */
  mixedVersions: boolean;
}

export function groupServersByHost(
  servers: Types.ServerListItem[],
): ServerHost[] {
  const groups = new Map<string, Types.ServerListItem[]>();
  for (const server of servers) {
    const key = server.info.host_name || server.name;
    const group = groups.get(key);
    if (group) {
      group.push(server);
    } else {
      groups.set(key, [server]);
    }
  }
  return [...groups.entries()]
    .map(([name, servers]) => {
      servers.sort((a, b) => {
        const au = a.info.rootless_user;
        const bu = b.info.rootless_user;
        if (!au && bu) return -1;
        if (au && !bu) return 1;
        return (au ?? a.name).localeCompare(bu ?? b.name);
      });
      const root = servers.find((s) => !s.info.rootless_user);
      const primary =
        (root?.info.stats ? root : undefined) ??
        servers.find((s) => s.info.stats) ??
        servers[0];
      const states = servers.map((s) => s.info.state);
      const state = states.includes(Types.ServerState.NotOk)
        ? Types.ServerState.NotOk
        : states.includes(Types.ServerState.Ok)
          ? Types.ServerState.Ok
          : Types.ServerState.Disabled;
      const versions = new Set(
        servers.map((s) => s.info.version).filter((v) => v),
      );
      return {
        name,
        servers,
        primary,
        state,
        mixedVersions: versions.size > 1,
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function agentLabel(server: Types.ServerListItem) {
  return server.info.rootless_user ?? "root";
}

/** Names of the Stacks deployed to each Server, by Server id. */
export function useStacksByServer() {
  const stacks = useRead("ListStacks", { limit: 0 }).data;
  return useMemo(() => {
    const map = new Map<string, string[]>();
    for (const stack of stacks ?? []) {
      const names = map.get(stack.info.server_id);
      if (names) {
        names.push(stack.name);
      } else {
        map.set(stack.info.server_id, [stack.name]);
      }
    }
    for (const names of map.values()) names.sort();
    return map;
  }, [stacks]);
}

export function hostStackCount(
  host: ServerHost,
  stacksByServer: Map<string, string[]>,
) {
  return host.servers.reduce(
    (count, s) => count + (stacksByServer.get(s.id)?.length ?? 0),
    0,
  );
}

export function memPerc(stats: Types.MinimalSystemStats | undefined) {
  return stats?.mem_total_gb
    ? (stats.mem_used_gb / stats.mem_total_gb) * 100
    : undefined;
}

export function diskPerc(stats: Types.MinimalSystemStats | undefined) {
  return stats?.disk_total_gb
    ? (stats.disk_used_gb / stats.disk_total_gb) * 100
    : undefined;
}

/** The host views need every agent of a host on one page,
 * so they list Servers without pagination. */
export function useUnpaginatedServerList(type: string) {
  const [view] = useServerView();
  return type === "Server" && view !== "agents";
}

/** The host's color: its state, or a warning when an agent's
 * version differs from Core's. */
export function hostIntention(host: ServerHost, coreVersion?: string) {
  return serverStateIntention(
    host.state,
    !!coreVersion &&
      host.servers.some(
        (s) => !!s.info.version && s.info.version !== coreVersion,
      ),
  );
}
