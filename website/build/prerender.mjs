// vinext는 요청 때 HTML을 렌더하는 Cloudflare Worker(dist/server/index.js)를 만든다.
// Vercel은 그 Worker를 못 돌리므로, 빌드 후 Worker를 한 번 실행해 완성된 HTML을
// index.html로 뽑고 정적 자원과 함께 out/에 모아 정적 배포한다.
// 이 랜딩 페이지는 요청별 동적 데이터가 없어(RSC 페이로드는 HTML에 인라인됨) 정적화가 안전하다.
import { access, cp, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dist = resolve(root, "dist");
const out = resolve(root, "out");

// og:image·아이콘 절대 URL에 쓸 배포 호스트.
// Vercel 빌드가 프로덕션 도메인을 이 env로 주입한다(로컬 실행 시 SITE_HOST 또는 기본값).
const host =
  process.env.VERCEL_PROJECT_PRODUCTION_URL ||
  process.env.SITE_HOST ||
  "elsecrew.dev";

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

const workerUrl = pathToFileURL(resolve(dist, "server/index.js")).href;
const { default: worker } = await import(workerUrl);

const response = await worker.fetch(
  new Request(`https://${host}/`, {
    headers: { accept: "text/html", host },
  }),
  {
    // 정적 자원은 out/에서 서빙되므로 Worker의 ASSETS는 미사용(스텁).
    ASSETS: { fetch: async () => new Response("Not found", { status: 404 }) },
  },
  { waitUntil() {}, passThroughOnException() {} },
);

if (response.status !== 200) {
  throw new Error(`prerender: 예상치 못한 상태 코드 ${response.status}`);
}
const html = await response.text();
if (!html.includes("Elsecrew")) {
  throw new Error("prerender: 렌더된 HTML에 핵심 문구가 없음");
}

await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });

// 클라이언트 정적 자원(assets/, og.png 등) 그대로 복사 → 하이드레이션 스크립트가 동작.
await cp(resolve(dist, "client"), out, { recursive: true });

// app/icon.png·apple-icon.png는 원래 Worker 라우트(/icon.png)로 서빙되므로,
// 정적 배포에서는 원본 파일을 복사해 같은 경로로 접근되게 한다(쿼리 해시는 무시됨).
for (const icon of ["icon.png", "apple-icon.png", "favicon.ico"]) {
  const src = resolve(root, "app", icon);
  if (await exists(src)) {
    await cp(src, resolve(out, icon));
  }
}

await writeFile(resolve(out, "index.html"), html, "utf8");

console.log(`prerender: out/index.html 생성 (${html.length} bytes, host=${host})`);
