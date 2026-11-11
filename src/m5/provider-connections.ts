import { validateProjectCatalogResult } from './contracts/project-catalog-v1.ts';
export const CONNECTIONS_CHANGED = 'm5-provider-connections-changed';
export type Connections = {codex:boolean;'claude-code':boolean};
export async function readConnections(): Promise<Connections> {
  if (!window.m5Projects) throw Error('connections unavailable');
  const request = {action:'getProviderConnections'} as const;
  const result = validateProjectCatalogResult(await window.m5Projects.request(request),request);
  if (result.action !== request.action) throw Error('invalid connections');
  return result.connections;
}
export async function changeConnection(runtime:keyof Connections,connected:boolean): Promise<Connections> {
  if (!window.m5Projects) throw Error('connections unavailable');
  const request = {action:'setProviderConnection',runtime,connected} as const;
  const result=validateProjectCatalogResult(await window.m5Projects.request(request),request);
  if(result.action!==request.action) throw Error('invalid connections');
  window.dispatchEvent(new Event(CONNECTIONS_CHANGED));
  return result.connections;
}
