import { load, type Store } from "@tauri-apps/plugin-store";

// 홈베이스 영속 데이터. 에이전트 설치·로그인 상태는 저장하지 않고 매번 실시간 감지한다
// (사용자가 앱 밖에서 바꿀 수 있으므로 — 진실은 항상 시스템). docs/architecture.md §3.
export interface SavedProject {
  path: string;
  agent: string;
  name: string;
  createdAt: number;
  lastOpenedAt: number;
}

const FILE = "elsecrew.json";
const PROJECTS_KEY = "projects";
const BASE_DIR_KEY = "baseDir";

let storePromise: Promise<Store> | null = null;
function getStore(): Promise<Store> {
  if (!storePromise) storePromise = load(FILE, { autoSave: true });
  return storePromise;
}

/** 프로젝트를 나열할 기준 폴더. 미설정이면 null(호출부가 기본값=Documents로 채운다). */
export async function getBaseDir(): Promise<string | null> {
  try {
    const store = await getStore();
    return (await store.get<string>(BASE_DIR_KEY)) ?? null;
  } catch {
    return null;
  }
}

export async function setBaseDir(dir: string): Promise<void> {
  try {
    const store = await getStore();
    await store.set(BASE_DIR_KEY, dir);
  } catch {
    // 무시
  }
}

/** 경로별 마지막 사용 시각(정렬용). store에 기록이 있으면 반환. */
export async function lastOpenedMap(): Promise<Record<string, number>> {
  const list = await listProjects();
  const map: Record<string, number> = {};
  for (const p of list) map[p.path] = p.lastOpenedAt;
  return map;
}

export async function listProjects(): Promise<SavedProject[]> {
  try {
    const store = await getStore();
    const projects = (await store.get<SavedProject[]>(PROJECTS_KEY)) ?? [];
    // 최근 사용 순
    return [...projects].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  } catch {
    return [];
  }
}

/** 프로젝트를 기록한다. 같은 경로가 있으면 갱신(중복 방지). */
export async function saveProject(
  p: Omit<SavedProject, "createdAt" | "lastOpenedAt"> &
    Partial<Pick<SavedProject, "createdAt" | "lastOpenedAt">>,
): Promise<void> {
  try {
    const store = await getStore();
    const now = Date.now();
    const existing = (await store.get<SavedProject[]>(PROJECTS_KEY)) ?? [];
    const prev = existing.find((e) => e.path === p.path);
    const merged: SavedProject = {
      path: p.path,
      agent: p.agent,
      name: p.name,
      createdAt: prev?.createdAt ?? p.createdAt ?? now,
      lastOpenedAt: p.lastOpenedAt ?? now,
    };
    const next = [merged, ...existing.filter((e) => e.path !== p.path)];
    await store.set(PROJECTS_KEY, next);
  } catch {
    // 저장 실패는 치명적이지 않다 — 이번 세션은 그대로 진행
  }
}

export async function touchProject(path: string): Promise<void> {
  try {
    const store = await getStore();
    const existing = (await store.get<SavedProject[]>(PROJECTS_KEY)) ?? [];
    const next = existing.map((e) =>
      e.path === path ? { ...e, lastOpenedAt: Date.now() } : e,
    );
    await store.set(PROJECTS_KEY, next);
  } catch {
    // 무시
  }
}

export async function removeProject(path: string): Promise<void> {
  try {
    const store = await getStore();
    const existing = (await store.get<SavedProject[]>(PROJECTS_KEY)) ?? [];
    await store.set(
      PROJECTS_KEY,
      existing.filter((e) => e.path !== path),
    );
  } catch {
    // 무시
  }
}
